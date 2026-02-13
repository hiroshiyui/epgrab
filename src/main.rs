use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read as _, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
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
        Some("serve") => cmd_serve(&args[2..]),
        _ => print_usage(),
    }
}

fn print_usage() {
    eprintln!("Usage: epgrab <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  run              Grab EPG data from DVB-T tuner device");
    eprintln!("  save-xmltv       Save EPG data as XMLTV files");
    eprintln!("  serve            Serve XMLTV files over HTTP");
    eprintln!("  scan-channels    Scan for available channels");
    eprintln!("  doctor           Check system readiness");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  epgrab run");
    eprintln!("  epgrab save-xmltv");
    eprintln!("  epgrab serve -b 0.0.0.0 -p 8080 --public");
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

fn cmd_serve(args: &[String]) {
    let mut bind = "127.0.0.1".to_string();
    let mut port: u16 = 3000;
    let mut public = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-b" | "--bind" => {
                bind = args.get(i + 1).cloned().unwrap_or_else(|| {
                    eprintln!("Error: missing value for {}", args[i]);
                    process::exit(1);
                });
                i += 2;
            }
            "-p" | "--port" => {
                let port_str = args.get(i + 1).cloned().unwrap_or_else(|| {
                    eprintln!("Error: missing value for {}", args[i]);
                    process::exit(1);
                });
                port = port_str.parse::<u16>().unwrap_or_else(|_| {
                    eprintln!("Error: invalid port number '{port_str}' (must be 1-65535)");
                    process::exit(1);
                });
                if port == 0 {
                    eprintln!("Error: invalid port number '0' (must be 1-65535)");
                    process::exit(1);
                }
                i += 2;
            }
            "--public" => {
                public = true;
                i += 1;
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                eprintln!("Usage: epgrab serve [-b <bind>] [-p <port>] [--public]");
                process::exit(1);
            }
        }
    }

    // Require --public for non-loopback bind addresses
    let is_loopback = bind == "127.0.0.1" || bind == "::1" || bind == "localhost";
    if !is_loopback && !public {
        eprintln!(
            "Error: binding to '{bind}' exposes the server to the network."
        );
        eprintln!("If this is intentional, add the --public flag.");
        process::exit(1);
    }

    let epg_dir = Path::new("epg");
    if !epg_dir.is_dir() {
        eprintln!("epg/ directory not found. Run 'epgrab save-xmltv' first.");
        process::exit(1);
    }

    let addr = format!("{}:{}", bind, port);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {addr}: {e}");
            process::exit(1);
        }
    };

    // Set up graceful shutdown on SIGINT/SIGTERM
    let _ = unsafe { libc::signal(libc::SIGINT, serve_signal_handler as *const () as libc::sighandler_t) };
    let _ = unsafe { libc::signal(libc::SIGTERM, serve_signal_handler as *const () as libc::sighandler_t) };

    // Use non-blocking accept so we can check the shutdown flag periodically
    listener
        .set_nonblocking(true)
        .expect("Failed to set non-blocking mode");

    eprintln!("Serving epg/ at http://{addr}/");

    while !SERVE_SHUTDOWN.load(Ordering::Relaxed) {
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => {
                eprintln!("Connection error: {e}");
                continue;
            }
        };

        let timeout = Some(Duration::from_secs(10));
        let _ = stream.set_read_timeout(timeout);
        let _ = stream.set_write_timeout(timeout);

        // Limit request line to 8 KiB to prevent memory exhaustion
        const MAX_REQUEST_LINE: u64 = 8192;
        let mut limited = BufReader::new((&stream).take(MAX_REQUEST_LINE));
        let mut request_line = String::new();
        match limited.read_line(&mut request_line) {
            Ok(0) | Err(_) => continue,
            Ok(_) => {}
        }

        handle_request(&mut stream, request_line.trim_end(), epg_dir);
    }

    eprintln!("\nShutting down.");
}

static SERVE_SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn serve_signal_handler(_sig: libc::c_int) {
    SERVE_SHUTDOWN.store(true, Ordering::Relaxed);
}

fn handle_request(stream: &mut impl Write, request_line: &str, epg_dir: &Path) {
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "GET" {
        let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
        return;
    }

    let raw_path = parts[1];

    // Decode percent-encoded path
    let decoded_path = percent_decode(raw_path);

    // Strip query string
    let path = decoded_path.split('?').next().unwrap_or(&decoded_path);

    // Reject path traversal
    if path.contains("..") || !path.starts_with('/') {
        let _ = stream.write_all(
            b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid path\n",
        );
        return;
    }

    if path == "/" {
        // Directory listing
        let mut entries: Vec<String> = Vec::new();
        if let Ok(dir) = std::fs::read_dir(epg_dir) {
            for entry in dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".xml") || name.ends_with(".xsl") {
                    entries.push(name);
                }
            }
        }
        entries.sort();

        let mut body = String::from(
            "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>EPG Files</title></head>\n<body>\n<h1>EPG Files</h1>\n<ul>\n",
        );
        for name in &entries {
            let escaped = xml_escape(name);
            body.push_str(&format!("  <li><a href=\"/{escaped}\">{escaped}</a></li>\n"));
        }
        body.push_str("</ul>\n</body></html>\n");

        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(body.as_bytes());
    } else {
        // Serve a file from epg/
        let filename = &path[1..]; // strip leading '/'

        // Only allow simple filenames (no subdirectory traversal)
        if filename.contains('/') || filename.contains('\\') || filename.is_empty() {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\nNot found\n",
            );
            return;
        }

        let file_path = epg_dir.join(filename);

        // Prevent symlink escape: verify resolved path stays within epg_dir
        if let Ok(canonical_epg) = epg_dir.canonicalize() {
            if let Ok(canonical_file) = file_path.canonicalize() {
                if !canonical_file.starts_with(&canonical_epg) {
                    let _ = stream.write_all(
                        b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\nForbidden\n",
                    );
                    return;
                }
            }
        }

        match std::fs::read(&file_path) {
            Ok(contents) => {
                let content_type = if filename.ends_with(".xml") || filename.ends_with(".xsl")
                {
                    "application/xml; charset=utf-8"
                } else {
                    "application/octet-stream"
                };

                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    contents.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&contents);
            }
            Err(_) => {
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\nNot found\n",
                );
            }
        }
    }
}

fn percent_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                &String::from_utf8_lossy(&bytes[i + 1..i + 3]),
                16,
            ) {
                result.push(byte);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).to_string()
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

    // --- percent_decode ---

    #[test]
    fn test_percent_decode_plain() {
        assert_eq!(percent_decode("/hello"), "/hello");
    }

    #[test]
    fn test_percent_decode_space() {
        assert_eq!(percent_decode("/hello%20world"), "/hello world");
    }

    #[test]
    fn test_percent_decode_cjk() {
        // テスト (katakana "tesuto") in UTF-8 is E3 83 86 E3 82 B9 E3 83 88
        assert_eq!(
            percent_decode("/%E3%83%86%E3%82%B9%E3%83%88.eit.xml"),
            "/テスト.eit.xml"
        );
    }

    #[test]
    fn test_percent_decode_invalid_hex() {
        assert_eq!(percent_decode("%ZZ"), "%ZZ");
    }

    #[test]
    fn test_percent_decode_truncated() {
        assert_eq!(percent_decode("abc%"), "abc%");
        assert_eq!(percent_decode("abc%A"), "abc%A");
    }

    // --- handle_request ---

    fn response_str(request_line: &str, epg_dir: &Path) -> String {
        let mut buf: Vec<u8> = Vec::new();
        handle_request(&mut buf, request_line, epg_dir);
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn test_serve_root_listing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.eit.xml"), "<tv/>").unwrap();
        std::fs::write(dir.path().join("epg.xsl"), "<xsl/>").unwrap();
        std::fs::write(dir.path().join("readme.txt"), "ignore me").unwrap();

        let resp = response_str("GET / HTTP/1.1", dir.path());
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("Content-Type: text/html"));
        assert!(resp.contains("test.eit.xml"));
        assert!(resp.contains("epg.xsl"));
        assert!(!resp.contains("readme.txt"));
    }

    #[test]
    fn test_serve_xml_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ch1.eit.xml"), "<tv>data</tv>").unwrap();

        let resp = response_str("GET /ch1.eit.xml HTTP/1.1", dir.path());
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("Content-Type: application/xml"));
        assert!(resp.contains("<tv>data</tv>"));
    }

    #[test]
    fn test_serve_xsl_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("epg.xsl"), "<stylesheet/>").unwrap();

        let resp = response_str("GET /epg.xsl HTTP/1.1", dir.path());
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("Content-Type: application/xml"));
        assert!(resp.contains("<stylesheet/>"));
    }

    #[test]
    fn test_serve_404() {
        let dir = tempfile::tempdir().unwrap();

        let resp = response_str("GET /nonexistent.xml HTTP/1.1", dir.path());
        assert!(resp.starts_with("HTTP/1.1 404 Not Found"));
    }

    #[test]
    fn test_serve_path_traversal_dotdot() {
        let dir = tempfile::tempdir().unwrap();

        let resp = response_str("GET /../etc/passwd HTTP/1.1", dir.path());
        assert!(resp.starts_with("HTTP/1.1 400 Bad Request"));
    }

    #[test]
    fn test_serve_path_traversal_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("file.xml"), "<tv/>").unwrap();

        let resp = response_str("GET /sub/file.xml HTTP/1.1", dir.path());
        assert!(resp.starts_with("HTTP/1.1 404 Not Found"));
    }

    #[test]
    fn test_serve_bad_method() {
        let dir = tempfile::tempdir().unwrap();

        let resp = response_str("POST / HTTP/1.1", dir.path());
        assert!(resp.starts_with("HTTP/1.1 400 Bad Request"));
    }

    #[test]
    fn test_serve_percent_encoded_filename() {
        let dir = tempfile::tempdir().unwrap();
        // テスト (katakana "tesuto") as a pseudo channel name
        std::fs::write(dir.path().join("テスト.eit.xml"), "<tv>test</tv>").unwrap();

        let resp = response_str(
            "GET /%E3%83%86%E3%82%B9%E3%83%88.eit.xml HTTP/1.1",
            dir.path(),
        );
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("<tv>test</tv>"));
    }

    #[cfg(unix)]
    #[test]
    fn test_serve_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        // Create a file outside the served directory
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.xml"), "sensitive data").unwrap();

        // Create a symlink inside the served dir pointing outside
        std::os::unix::fs::symlink(
            outside.path().join("secret.xml"),
            dir.path().join("secret.xml"),
        )
        .unwrap();

        let resp = response_str("GET /secret.xml HTTP/1.1", dir.path());
        assert!(
            resp.starts_with("HTTP/1.1 403 Forbidden"),
            "Symlink escape should be blocked, got: {resp}"
        );
        assert!(!resp.contains("sensitive data"));
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
