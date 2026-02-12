use std::collections::BTreeMap;
use std::path::Path;
use std::process;

use epgrab::channel::Channel;
use epgrab::dvb_device;
use epgrab::eit;
use epgrab::channel;
use epgrab::scan;
use epgrab::tuner;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("run") => cmd_run(),
        Some("scan-channels") => cmd_scan_channels(&args[2..]),
        Some("doctor") => cmd_doctor(),
        Some("save-xmltv") => cmd_save_xmltv(),
        _ => print_usage(),
    }
}

fn print_usage() {
    eprintln!("Usage: epgrab <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  run              Grab EPG data from DVB-T tuner device");
    eprintln!("  save-xmltv       Save EPG data as XMLTV files");
    eprintln!("  scan-channels    Scan for available channels");
    eprintln!("  doctor           Check system readiness");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  epgrab run");
    eprintln!("  epgrab save-xmltv");
    eprintln!("  epgrab scan-channels -C /usr/share/dvb/dvb-t/tw-All");
    eprintln!("  epgrab doctor");
    process::exit(1);
}

fn cmd_run() {
    let devices = dvb_device::detect_devices();

    if devices.is_empty() {
        eprintln!("No DVB-T devices found.");
        process::exit(1);
    }

    // Extract adapter number from first device (e.g., "dvb0.frontend0" → 0)
    let adapter: u32 = devices[0]
        .adapter_name
        .strip_prefix("dvb")
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    for dev in &devices {
        let vendor_display = dev.vendor_name.as_deref().unwrap_or("Unknown vendor");
        let product_display = dev.product_name.as_deref().unwrap_or("Unknown device");
        println!(
            "{}: {} - {} (vendor={}, device={})",
            dev.adapter_name, vendor_display, product_display, dev.vendor_id, dev.device_id
        );
    }

    println!();

    let conf_path = Path::new("etc/channels.conf");
    let channels = match channel::parse_channels_conf(conf_path) {
        Ok(channels) => {
            println!("Loaded {} channels.", channels.len());
            channels
        }
        Err(e) => {
            eprintln!("Error parsing channels.conf: {e}");
            process::exit(1);
        }
    };

    // Group channels by frequency (same multiplex shares EIT data)
    let mut freq_groups: BTreeMap<u64, Vec<&Channel>> = BTreeMap::new();
    for ch in &channels {
        freq_groups.entry(ch.frequency).or_default().push(ch);
    }

    // Open tuner
    let tuner = match tuner::Tuner::open(adapter) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to open tuner: {e}");
            process::exit(1);
        }
    };

    let num_freqs = freq_groups.len();
    for (i, (freq, group)) in freq_groups.iter().enumerate() {
        println!(
            "[{}/{}] Tuning to {} MHz ({} channels)...",
            i + 1,
            num_freqs,
            freq / 1_000_000,
            group.len(),
        );

        // Tune using the first channel in the group (same tuning params for all)
        if let Err(e) = tuner.tune(group[0]) {
            eprintln!("  Skipped: {e}");
            println!();
            continue;
        }

        // Open demux after tuning
        let mut eit_reader = match eit::EitReader::open(adapter) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  Failed to open demux: {e}");
                println!();
                continue;
            }
        };

        if !tuner.has_lock() {
            eprintln!("  Warning: frontend lost lock before EIT reading");
        }

        // Read EIT data (30-second timeout)
        println!("  Reading EIT data...");
        match eit_reader.read_events(30) {
            Ok(events) => {
                if events.is_empty() {
                    println!("  No EIT events received.");
                } else {
                    // Group events by service_id and map to channel name
                    for ch in group {
                        let ch_events: Vec<_> = events
                            .iter()
                            .filter(|e| e.service_id == ch.service_id)
                            .collect();

                        if ch_events.is_empty() {
                            continue;
                        }

                        println!("  {} (SID={}):", ch.name, ch.service_id);
                        for event in &ch_events {
                            let start = format_unix_timestamp(event.start_time);
                            let dur_h = event.duration / 3600;
                            let dur_m = (event.duration % 3600) / 60;
                            println!(
                                "    [{}] {} ({}h{}m) - {} [{}]",
                                event.event_id,
                                event.event_name,
                                dur_h,
                                dur_m,
                                start,
                                event.language,
                            );
                            if !event.description.is_empty() {
                                println!("      {}", event.description);
                            }
                        }
                    }

                    // Show events for services not in channels.conf
                    let known_sids: Vec<u16> = group.iter().map(|ch| ch.service_id).collect();
                    let unknown: Vec<_> = events
                        .iter()
                        .filter(|e| !known_sids.contains(&e.service_id))
                        .collect();
                    if !unknown.is_empty() {
                        println!("  Unknown services:");
                        for event in &unknown {
                            let start = format_unix_timestamp(event.start_time);
                            let dur_h = event.duration / 3600;
                            let dur_m = (event.duration % 3600) / 60;
                            println!(
                                "    SID={}: [{}] {} ({}h{}m) - {} [{}]",
                                event.service_id,
                                event.event_id,
                                event.event_name,
                                dur_h,
                                dur_m,
                                start,
                                event.language,
                            );
                        }
                    }
                }
            }
            Err(e) => eprintln!("  Failed to read EIT: {e}"),
        }
        println!();
    }
}

fn cmd_save_xmltv() {
    let devices = dvb_device::detect_devices();

    if devices.is_empty() {
        eprintln!("No DVB-T devices found.");
        process::exit(1);
    }

    let adapter: u32 = devices[0]
        .adapter_name
        .strip_prefix("dvb")
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let conf_path = Path::new("etc/channels.conf");
    let channels = match channel::parse_channels_conf(conf_path) {
        Ok(channels) => {
            println!("Loaded {} channels.", channels.len());
            channels
        }
        Err(e) => {
            eprintln!("Error parsing channels.conf: {e}");
            process::exit(1);
        }
    };

    // Group channels by frequency
    let mut freq_groups: BTreeMap<u64, Vec<&Channel>> = BTreeMap::new();
    for ch in &channels {
        freq_groups.entry(ch.frequency).or_default().push(ch);
    }

    // Open tuner
    let tuner = match tuner::Tuner::open(adapter) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to open tuner: {e}");
            process::exit(1);
        }
    };

    // Create output directory
    if let Err(e) = std::fs::create_dir_all("epg") {
        eprintln!("Failed to create epg/ directory: {e}");
        process::exit(1);
    }

    // Collect all events keyed by channel name
    let mut channel_events: BTreeMap<String, (u16, Vec<eit::EitEvent>)> = BTreeMap::new();
    for ch in &channels {
        channel_events.insert(ch.name.clone(), (ch.service_id, Vec::new()));
    }

    let num_freqs = freq_groups.len();
    for (i, (freq, group)) in freq_groups.iter().enumerate() {
        println!(
            "[{}/{}] Tuning to {} MHz ({} channels)...",
            i + 1,
            num_freqs,
            freq / 1_000_000,
            group.len(),
        );

        if let Err(e) = tuner.tune(group[0]) {
            eprintln!("  Skipped: {e}");
            println!();
            continue;
        }

        let mut eit_reader = match eit::EitReader::open(adapter) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  Failed to open demux: {e}");
                println!();
                continue;
            }
        };

        if !tuner.has_lock() {
            eprintln!("  Warning: frontend lost lock before EIT reading");
        }

        println!("  Reading EIT data...");
        match eit_reader.read_events(30) {
            Ok(events) => {
                let event_count = events.len();
                for event in events {
                    for ch in group {
                        if event.service_id == ch.service_id {
                            if let Some((_, evts)) = channel_events.get_mut(&ch.name) {
                                evts.push(event);
                                break;
                            }
                        }
                    }
                }
                println!("  Received {event_count} events.");
            }
            Err(e) => eprintln!("  Failed to read EIT: {e}"),
        }
        println!();
    }

    // Check if XSLT stylesheet exists
    let use_xslt = Path::new("epg/epg.xsl").exists();
    if use_xslt {
        println!("Found epg/epg.xsl, linking stylesheet in XML files.");
    }

    // Write XMLTV files
    let mut files_written = 0;
    for (name, (_sid, events)) in &channel_events {
        if events.is_empty() {
            continue;
        }

        let safe_name = sanitize_filename(name);
        let filename = format!("epg/{}.eit.xml", safe_name);
        let xml = generate_xmltv(name, events, use_xslt);

        match std::fs::write(&filename, &xml) {
            Ok(()) => {
                println!("Wrote {} ({} events)", filename, events.len());
                files_written += 1;
            }
            Err(e) => eprintln!("Failed to write {filename}: {e}"),
        }
    }

    println!("\nSaved {files_written} XMLTV files to epg/");
}

fn generate_xmltv(channel_name: &str, events: &[eit::EitEvent], use_xslt: bool) -> String {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    if use_xslt {
        xml.push_str("<?xml-stylesheet type=\"text/xsl\" href=\"epg.xsl\"?>\n");
    }
    xml.push_str("<!DOCTYPE tv SYSTEM \"xmltv.dtd\">\n");
    xml.push_str("<tv generator-info-name=\"epgrab\">\n");

    // Channel element
    let channel_id = xml_escape(channel_name);
    xml.push_str(&format!(
        "  <channel id=\"{channel_id}\">\n    <display-name>{channel_id}</display-name>\n  </channel>\n"
    ));

    // Programme elements
    for event in events {
        let start = format_xmltv_time(event.start_time);
        let stop = format_xmltv_time(event.start_time + event.duration as i64);
        let title = xml_escape(&event.event_name);
        let lang = if event.language.is_empty() {
            String::new()
        } else {
            format!(" lang=\"{}\"", xml_escape(&event.language))
        };

        xml.push_str(&format!(
            "  <programme start=\"{start}\" stop=\"{stop}\" channel=\"{channel_id}\">\n"
        ));
        xml.push_str(&format!("    <title{lang}>{title}</title>\n"));

        if !event.description.is_empty() {
            let desc = xml_escape(&event.description);
            xml.push_str(&format!("    <desc{lang}>{desc}</desc>\n"));
        }

        xml.push_str("  </programme>\n");
    }

    xml.push_str("</tv>\n");
    xml
}

fn format_xmltv_time(ts: i64) -> String {
    let time_t = ts as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&time_t, &mut tm) };

    let offset_secs = tm.tm_gmtoff;
    let offset_h = offset_secs.abs() / 3600;
    let offset_m = (offset_secs.abs() % 3600) / 60;
    let sign = if offset_secs >= 0 { '+' } else { '-' };

    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02} {}{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        sign,
        offset_h,
        offset_m,
    )
}

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | '\0' => '_',
            '.' if s.starts_with('.') => '_',
            _ => c,
        })
        .collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn cmd_doctor() {
    const GREEN: &str = "\x1b[32m";
    const RED: &str = "\x1b[31m";
    const BOLD: &str = "\x1b[1m";
    const RESET: &str = "\x1b[0m";

    let mut ok = true;

    // 1. Check DVB-T device
    print!("DVB-T device ... ");
    let devices = dvb_device::detect_devices();
    if devices.is_empty() {
        println!("{RED}{BOLD}NOT FOUND{RESET}");
        ok = false;
    } else {
        let dev = &devices[0];
        let vendor = dev.vendor_name.as_deref().unwrap_or("Unknown vendor");
        let product = dev.product_name.as_deref().unwrap_or("Unknown device");
        println!("{GREEN}OK{RESET} ({}: {} - {})", dev.adapter_name, vendor, product);
    }

    // 2. Check etc/channels.conf
    print!("etc/channels.conf ... ");
    let conf_path = Path::new("etc/channels.conf");
    if !conf_path.exists() {
        println!("{RED}{BOLD}NOT FOUND{RESET}");
        println!("  Run 'epgrab scan-channels -C <scan-file>' to create it.");
        ok = false;
    } else {
        match channel::parse_channels_conf(conf_path) {
            Ok(channels) if channels.is_empty() => {
                println!("{RED}{BOLD}EMPTY{RESET} (no channels)");
                ok = false;
            }
            Ok(channels) => {
                println!("{GREEN}OK{RESET} ({} channels)", channels.len());
            }
            Err(e) => {
                println!("{RED}{BOLD}INVALID{RESET}");
                println!("  {e}");
                ok = false;
            }
        }
    }

    println!();
    if ok {
        println!("{GREEN}{BOLD}All checks passed.{RESET}");
    } else {
        println!("{RED}{BOLD}Some checks failed.{RESET}");
        process::exit(1);
    }
}

fn cmd_scan_channels(args: &[String]) {
    let config_path = match args.iter().position(|a| a == "-C" || a == "--config") {
        Some(i) => match args.get(i + 1) {
            Some(path) => path.clone(),
            None => {
                eprintln!("Error: missing value for {}", args[i]);
                eprintln!("Usage: epgrab scan-channels -C <file> | --config <file>");
                process::exit(1);
            }
        },
        None => {
            eprintln!("Error: -C or --config is required");
            eprintln!("Usage: epgrab scan-channels -C <file> | --config <file>");
            process::exit(1);
        }
    };

    // Detect DVB device
    let devices = dvb_device::detect_devices();
    if devices.is_empty() {
        eprintln!("No DVB-T devices found.");
        process::exit(1);
    }

    let adapter: u32 = devices[0]
        .adapter_name
        .strip_prefix("dvb")
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let dev = &devices[0];
    let vendor_display = dev.vendor_name.as_deref().unwrap_or("Unknown vendor");
    let product_display = dev.product_name.as_deref().unwrap_or("Unknown device");
    println!(
        "Using {}: {} - {}",
        dev.adapter_name, vendor_display, product_display
    );

    // Parse scan file
    let entries = match scan::parse_scan_file(&config_path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    println!(
        "Scanning {} frequencies from {config_path}\n",
        entries.len()
    );

    // Open tuner
    let tuner = match tuner::Tuner::open(adapter) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to open tuner: {e}");
            process::exit(1);
        }
    };

    let mut all_channels: Vec<Channel> = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        println!(
            "[{}/{}] Tuning to {} MHz ({})...",
            i + 1,
            entries.len(),
            entry.frequency / 1_000_000,
            entry.modulation,
        );

        let tune_channel = entry.to_channel();
        if let Err(e) = tuner.tune(&tune_channel) {
            eprintln!("  Skipped: {e}");
            println!();
            continue;
        }

        match scan::scan_frequency(adapter, entry) {
            Ok(channels) => {
                println!("  Found {} services:", channels.len());
                for ch in &channels {
                    println!(
                        "    {} (SID={}, video={}, audio={})",
                        ch.name, ch.service_id, ch.video_pid, ch.audio_pid
                    );
                }
                all_channels.extend(channels);
            }
            Err(e) => {
                eprintln!("  Scan error: {e}");
            }
        }
        println!();
    }

    if all_channels.is_empty() {
        println!("No channels found.");
        return;
    }

    // Write etc/channels.conf
    let output_path = "etc/channels.conf";
    let mut content = String::new();
    for ch in &all_channels {
        content.push_str(&channel_to_zap_line(ch));
        content.push('\n');
    }

    if let Err(e) = std::fs::create_dir_all("etc") {
        eprintln!("Failed to create etc/ directory: {e}");
        process::exit(1);
    }

    // Back up existing channels.conf
    if Path::new(output_path).exists() {
        let backup_path = format!("{output_path}.old");
        if let Err(e) = std::fs::rename(output_path, &backup_path) {
            eprintln!("Failed to back up {output_path}: {e}");
            process::exit(1);
        }
        println!("Backed up existing {output_path} to {backup_path}");
    }

    match std::fs::write(output_path, &content) {
        Ok(()) => {
            println!(
                "Wrote {} channels to {output_path}",
                all_channels.len()
            );
        }
        Err(e) => {
            eprintln!("Failed to write {output_path}: {e}");
            process::exit(1);
        }
    }
}

fn channel_to_zap_line(ch: &Channel) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        ch.name,
        ch.frequency,
        ch.inversion,
        ch.bandwidth,
        ch.fec_hp,
        ch.fec_lp,
        ch.modulation,
        ch.transmission_mode,
        ch.guard_interval,
        ch.hierarchy,
        ch.video_pid,
        ch.audio_pid,
        ch.service_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- sanitize_filename ---

    #[test]
    fn test_sanitize_filename_normal() {
        assert_eq!(sanitize_filename("channel_name"), "channel_name");
    }

    #[test]
    fn test_sanitize_filename_slashes() {
        assert_eq!(sanitize_filename("a/b\\c"), "a_b_c");
    }

    #[test]
    fn test_sanitize_filename_null_byte() {
        assert_eq!(sanitize_filename("a\0b"), "a_b");
    }

    #[test]
    fn test_sanitize_filename_leading_dot() {
        assert_eq!(sanitize_filename(".hidden"), "_hidden");
    }

    #[test]
    fn test_sanitize_filename_non_leading_dot() {
        assert_eq!(sanitize_filename("file.xml"), "file.xml");
    }

    #[test]
    fn test_sanitize_filename_cjk() {
        assert_eq!(sanitize_filename("公視"), "公視");
    }

    #[test]
    fn test_sanitize_filename_path_traversal() {
        // Starts with '.', so all dots become '_'; '/' also becomes '_'
        assert_eq!(sanitize_filename("../../../etc/passwd"), "_________etc_passwd");
    }

    // --- xml_escape ---

    #[test]
    fn test_xml_escape_no_special() {
        assert_eq!(xml_escape("Hello World"), "Hello World");
    }

    #[test]
    fn test_xml_escape_ampersand() {
        assert_eq!(xml_escape("a&b"), "a&amp;b");
    }

    #[test]
    fn test_xml_escape_angle_brackets() {
        assert_eq!(xml_escape("<tag>"), "&lt;tag&gt;");
    }

    #[test]
    fn test_xml_escape_quotes() {
        assert_eq!(xml_escape("a\"b'c"), "a&quot;b&apos;c");
    }

    #[test]
    fn test_xml_escape_all_special() {
        assert_eq!(
            xml_escape("<>&\"'"),
            "&lt;&gt;&amp;&quot;&apos;"
        );
    }

    #[test]
    fn test_xml_escape_empty() {
        assert_eq!(xml_escape(""), "");
    }

    // --- format_xmltv_time ---

    #[test]
    fn test_format_xmltv_time_format() {
        // Just verify it produces a properly formatted string
        let result = format_xmltv_time(0);
        // Should match: YYYYMMDDHHmmSS +HHMM or -HHMM
        assert_eq!(result.len(), 20); // "19700101HHMMSS +HHMM"
        assert!(result.contains(' ')); // space between datetime and timezone
    }

    #[test]
    fn test_format_xmltv_time_known_timestamp() {
        // 946684800 = 2000-01-01 00:00:00 UTC
        let result = format_xmltv_time(946684800);
        // The output depends on local timezone, but should start with 2000
        assert!(result.starts_with("2000"));
    }

    // --- channel_to_zap_line ---

    #[test]
    fn test_channel_to_zap_line() {
        let ch = Channel {
            name: "公視".to_string(),
            frequency: 557000000,
            inversion: "INVERSION_AUTO".to_string(),
            bandwidth: "BANDWIDTH_6_MHZ".to_string(),
            fec_hp: "FEC_AUTO".to_string(),
            fec_lp: "FEC_AUTO".to_string(),
            modulation: "QAM_64".to_string(),
            transmission_mode: "TRANSMISSION_MODE_8K".to_string(),
            guard_interval: "GUARD_INTERVAL_1_8".to_string(),
            hierarchy: "HIERARCHY_NONE".to_string(),
            video_pid: 4097,
            audio_pid: 4098,
            service_id: 1,
        };
        let line = channel_to_zap_line(&ch);
        assert_eq!(
            line,
            "公視:557000000:INVERSION_AUTO:BANDWIDTH_6_MHZ:FEC_AUTO:FEC_AUTO:QAM_64:TRANSMISSION_MODE_8K:GUARD_INTERVAL_1_8:HIERARCHY_NONE:4097:4098:1"
        );
    }

    #[test]
    fn test_channel_to_zap_line_roundtrip() {
        // channel_to_zap_line output should be parseable by parse_channels_conf
        let ch = Channel {
            name: "TestCH".to_string(),
            frequency: 563000000,
            inversion: "INVERSION_AUTO".to_string(),
            bandwidth: "BANDWIDTH_6_MHZ".to_string(),
            fec_hp: "FEC_2_3".to_string(),
            fec_lp: "FEC_AUTO".to_string(),
            modulation: "QAM_64".to_string(),
            transmission_mode: "TRANSMISSION_MODE_8K".to_string(),
            guard_interval: "GUARD_INTERVAL_1_8".to_string(),
            hierarchy: "HIERARCHY_NONE".to_string(),
            video_pid: 100,
            audio_pid: 101,
            service_id: 42,
        };
        let line = channel_to_zap_line(&ch);
        let fields: Vec<&str> = line.split(':').collect();
        assert_eq!(fields.len(), 13);
        assert_eq!(fields[0], "TestCH");
        assert_eq!(fields[1], "563000000");
        assert_eq!(fields[12], "42");
    }

    // --- generate_xmltv ---

    #[test]
    fn test_generate_xmltv_basic() {
        let events = vec![eit::EitEvent {
            service_id: 1,
            event_id: 100,
            start_time: 946684800,  // 2000-01-01 00:00:00 UTC
            duration: 3600,
            running_status: 4,
            event_name: "Test Show".to_string(),
            description: "A test description".to_string(),
            language: "eng".to_string(),
        }];

        let xml = generate_xmltv("TestChannel", &events, false);
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<tv generator-info-name=\"epgrab\">"));
        assert!(xml.contains("<channel id=\"TestChannel\">"));
        assert!(xml.contains("<display-name>TestChannel</display-name>"));
        assert!(xml.contains("<title lang=\"eng\">Test Show</title>"));
        assert!(xml.contains("<desc lang=\"eng\">A test description</desc>"));
        assert!(xml.contains("</tv>"));
        assert!(!xml.contains("xml-stylesheet")); // no XSLT
    }

    #[test]
    fn test_generate_xmltv_with_xslt() {
        let events = vec![eit::EitEvent {
            service_id: 1,
            event_id: 100,
            start_time: 946684800,
            duration: 3600,
            running_status: 4,
            event_name: "Show".to_string(),
            description: String::new(),
            language: "eng".to_string(),
        }];

        let xml = generate_xmltv("CH1", &events, true);
        assert!(xml.contains("<?xml-stylesheet type=\"text/xsl\" href=\"epg.xsl\"?>"));
    }

    #[test]
    fn test_generate_xmltv_escapes_special_chars() {
        let events = vec![eit::EitEvent {
            service_id: 1,
            event_id: 1,
            start_time: 946684800,
            duration: 1800,
            running_status: 0,
            event_name: "A & B <Show>".to_string(),
            description: String::new(),
            language: String::new(),
        }];

        let xml = generate_xmltv("CH&1", &events, false);
        assert!(xml.contains("CH&amp;1"));
        assert!(xml.contains("A &amp; B &lt;Show&gt;"));
    }

    #[test]
    fn test_generate_xmltv_no_desc() {
        let events = vec![eit::EitEvent {
            service_id: 1,
            event_id: 1,
            start_time: 946684800,
            duration: 1800,
            running_status: 0,
            event_name: "Show".to_string(),
            description: String::new(),
            language: "eng".to_string(),
        }];

        let xml = generate_xmltv("CH1", &events, false);
        assert!(!xml.contains("<desc"));
    }

    #[test]
    fn test_generate_xmltv_no_language() {
        let events = vec![eit::EitEvent {
            service_id: 1,
            event_id: 1,
            start_time: 946684800,
            duration: 1800,
            running_status: 0,
            event_name: "Show".to_string(),
            description: String::new(),
            language: String::new(),
        }];

        let xml = generate_xmltv("CH1", &events, false);
        assert!(xml.contains("<title>Show</title>")); // no lang attr
    }

    #[test]
    fn test_generate_xmltv_empty_events() {
        let xml = generate_xmltv("CH1", &[], false);
        assert!(xml.contains("<channel id=\"CH1\">"));
        assert!(!xml.contains("<programme"));
    }
}

fn format_unix_timestamp(ts: i64) -> String {
    let time_t = ts as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&time_t, &mut tm) };

    let offset_secs = tm.tm_gmtoff;
    let offset_h = offset_secs / 3600;
    let offset_m = (offset_secs.abs() % 3600) / 60;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC{:+03}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        offset_h,
        offset_m,
    )
}
