use std::fs;
use std::io::Read;
use std::os::unix::io::AsRawFd;

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

use crate::channel::Channel;
use crate::dmx;
use crate::eit::decode_dvb_text;

// --- ScanEntry and dvbv5 file parsing ---

pub struct ScanEntry {
    pub delivery_system: String,
    pub frequency: u64,
    pub bandwidth_hz: u64,
    pub code_rate_hp: String,
    pub code_rate_lp: String,
    pub modulation: String,
    pub transmission_mode: String,
    pub guard_interval: String,
    pub hierarchy: String,
    pub inversion: String,
}

pub fn parse_scan_file(path: &str) -> Result<Vec<ScanEntry>, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read {path}: {e}"))?;

    let mut entries = Vec::new();
    let mut current: Option<ScanEntry> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "[CHANNEL]" {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(ScanEntry {
                delivery_system: String::new(),
                frequency: 0,
                bandwidth_hz: 0,
                code_rate_hp: String::new(),
                code_rate_lp: String::new(),
                modulation: String::new(),
                transmission_mode: String::new(),
                guard_interval: String::new(),
                hierarchy: String::new(),
                inversion: String::new(),
            });
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some(entry) = current.as_mut() else {
            continue;
        };

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };

        let key = key.trim();
        let value = value.trim();

        match key {
            "DELIVERY_SYSTEM" => entry.delivery_system = value.to_string(),
            "FREQUENCY" => {
                entry.frequency = value
                    .parse()
                    .map_err(|e| format!("Invalid FREQUENCY '{value}': {e}"))?;
            }
            "BANDWIDTH_HZ" => {
                entry.bandwidth_hz = value
                    .parse()
                    .map_err(|e| format!("Invalid BANDWIDTH_HZ '{value}': {e}"))?;
            }
            "CODE_RATE_HP" => entry.code_rate_hp = value.to_string(),
            "CODE_RATE_LP" => entry.code_rate_lp = value.to_string(),
            "MODULATION" => entry.modulation = value.to_string(),
            "TRANSMISSION_MODE" => entry.transmission_mode = value.to_string(),
            "GUARD_INTERVAL" => entry.guard_interval = value.to_string(),
            "HIERARCHY" => entry.hierarchy = value.to_string(),
            "INVERSION" => entry.inversion = value.to_string(),
            _ => {}
        }
    }

    if let Some(entry) = current {
        entries.push(entry);
    }

    Ok(entries)
}

// --- dvbv5 → zap format conversions ---

fn dvbv5_to_zap_inversion(s: &str) -> String {
    match s {
        "AUTO" => "INVERSION_AUTO",
        "ON" => "INVERSION_ON",
        "OFF" => "INVERSION_OFF",
        _ => "INVERSION_AUTO",
    }
    .to_string()
}

fn dvbv5_to_zap_bandwidth(hz: u64) -> String {
    match hz {
        5000000 => "BANDWIDTH_5_MHZ",
        6000000 => "BANDWIDTH_6_MHZ",
        7000000 => "BANDWIDTH_7_MHZ",
        8000000 => "BANDWIDTH_8_MHZ",
        10000000 => "BANDWIDTH_10_MHZ",
        1712000 => "BANDWIDTH_1_712_MHZ",
        _ => "BANDWIDTH_AUTO",
    }
    .to_string()
}

fn dvbv5_to_zap_fec(s: &str) -> String {
    match s {
        "NONE" => "FEC_NONE",
        "1/2" => "FEC_1_2",
        "2/3" => "FEC_2_3",
        "3/4" => "FEC_3_4",
        "4/5" => "FEC_4_5",
        "5/6" => "FEC_5_6",
        "6/7" => "FEC_6_7",
        "7/8" => "FEC_7_8",
        "8/9" => "FEC_8_9",
        "AUTO" => "FEC_AUTO",
        _ => "FEC_AUTO",
    }
    .to_string()
}

fn dvbv5_to_zap_modulation(s: &str) -> String {
    match s {
        "QPSK" => "QPSK",
        "QAM/16" => "QAM_16",
        "QAM/32" => "QAM_32",
        "QAM/64" => "QAM_64",
        "QAM/128" => "QAM_128",
        "QAM/256" => "QAM_256",
        "QAM/AUTO" => "QAM_AUTO",
        _ => "QAM_AUTO",
    }
    .to_string()
}

fn dvbv5_to_zap_transmission(s: &str) -> String {
    match s {
        "1K" => "TRANSMISSION_MODE_1K",
        "2K" => "TRANSMISSION_MODE_2K",
        "4K" => "TRANSMISSION_MODE_4K",
        "8K" => "TRANSMISSION_MODE_8K",
        "16K" => "TRANSMISSION_MODE_16K",
        "32K" => "TRANSMISSION_MODE_32K",
        "AUTO" => "TRANSMISSION_MODE_AUTO",
        _ => "TRANSMISSION_MODE_AUTO",
    }
    .to_string()
}

fn dvbv5_to_zap_guard(s: &str) -> String {
    match s {
        "1/32" => "GUARD_INTERVAL_1_32",
        "1/16" => "GUARD_INTERVAL_1_16",
        "1/8" => "GUARD_INTERVAL_1_8",
        "1/4" => "GUARD_INTERVAL_1_4",
        "AUTO" => "GUARD_INTERVAL_AUTO",
        _ => "GUARD_INTERVAL_AUTO",
    }
    .to_string()
}

fn dvbv5_to_zap_hierarchy(s: &str) -> String {
    match s {
        "NONE" => "HIERARCHY_NONE",
        "1" => "HIERARCHY_1",
        "2" => "HIERARCHY_2",
        "4" => "HIERARCHY_4",
        "AUTO" => "HIERARCHY_AUTO",
        _ => "HIERARCHY_NONE",
    }
    .to_string()
}

impl ScanEntry {
    /// Convert scan entry tuning params to a Channel (for use with Tuner::tune).
    /// Name/PIDs/service_id are left empty/zero.
    pub fn to_channel(&self) -> Channel {
        Channel {
            name: String::new(),
            frequency: self.frequency,
            inversion: dvbv5_to_zap_inversion(&self.inversion),
            bandwidth: dvbv5_to_zap_bandwidth(self.bandwidth_hz),
            fec_hp: dvbv5_to_zap_fec(&self.code_rate_hp),
            fec_lp: dvbv5_to_zap_fec(&self.code_rate_lp),
            modulation: dvbv5_to_zap_modulation(&self.modulation),
            transmission_mode: dvbv5_to_zap_transmission(&self.transmission_mode),
            guard_interval: dvbv5_to_zap_guard(&self.guard_interval),
            hierarchy: dvbv5_to_zap_hierarchy(&self.hierarchy),
            video_pid: 0,
            audio_pid: 0,
            service_id: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // --- parse_scan_file ---

    #[test]
    fn test_parse_scan_file_valid() {
        let content = "\
[CHANNEL]
DELIVERY_SYSTEM = DVBT
FREQUENCY = 557000000
BANDWIDTH_HZ = 6000000
CODE_RATE_HP = 2/3
CODE_RATE_LP = AUTO
MODULATION = QAM/64
TRANSMISSION_MODE = 8K
GUARD_INTERVAL = 1/8
HIERARCHY = NONE
INVERSION = AUTO
";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        let entries = parse_scan_file(f.path().to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].delivery_system, "DVBT");
        assert_eq!(entries[0].frequency, 557000000);
        assert_eq!(entries[0].bandwidth_hz, 6000000);
        assert_eq!(entries[0].code_rate_hp, "2/3");
        assert_eq!(entries[0].code_rate_lp, "AUTO");
        assert_eq!(entries[0].modulation, "QAM/64");
        assert_eq!(entries[0].transmission_mode, "8K");
        assert_eq!(entries[0].guard_interval, "1/8");
        assert_eq!(entries[0].hierarchy, "NONE");
        assert_eq!(entries[0].inversion, "AUTO");
    }

    #[test]
    fn test_parse_scan_file_multiple_channels() {
        let content = "\
[CHANNEL]
DELIVERY_SYSTEM = DVBT
FREQUENCY = 557000000
BANDWIDTH_HZ = 6000000

[CHANNEL]
DELIVERY_SYSTEM = DVBT
FREQUENCY = 563000000
BANDWIDTH_HZ = 6000000
";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        let entries = parse_scan_file(f.path().to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].frequency, 557000000);
        assert_eq!(entries[1].frequency, 563000000);
    }

    #[test]
    fn test_parse_scan_file_skips_comments() {
        let content = "\
# This is a comment
[CHANNEL]
DELIVERY_SYSTEM = DVBT
FREQUENCY = 557000000
# inline comment
BANDWIDTH_HZ = 6000000
";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        let entries = parse_scan_file(f.path().to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].frequency, 557000000);
    }

    #[test]
    fn test_parse_scan_file_empty() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"").unwrap();
        let entries = parse_scan_file(f.path().to_str().unwrap()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_scan_file_invalid_frequency() {
        let content = "\
[CHANNEL]
FREQUENCY = notanumber
";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        assert!(parse_scan_file(f.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn test_parse_scan_file_nonexistent() {
        assert!(parse_scan_file("/nonexistent/file").is_err());
    }

    #[test]
    fn test_parse_scan_file_ignores_unknown_keys() {
        let content = "\
[CHANNEL]
DELIVERY_SYSTEM = DVBT
FREQUENCY = 557000000
UNKNOWN_KEY = some_value
BANDWIDTH_HZ = 6000000
";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        let entries = parse_scan_file(f.path().to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].frequency, 557000000);
    }

    // --- dvbv5_to_zap conversions ---

    #[test]
    fn test_dvbv5_to_zap_inversion() {
        assert_eq!(dvbv5_to_zap_inversion("AUTO"), "INVERSION_AUTO");
        assert_eq!(dvbv5_to_zap_inversion("ON"), "INVERSION_ON");
        assert_eq!(dvbv5_to_zap_inversion("OFF"), "INVERSION_OFF");
        assert_eq!(dvbv5_to_zap_inversion("unknown"), "INVERSION_AUTO");
    }

    #[test]
    fn test_dvbv5_to_zap_bandwidth() {
        assert_eq!(dvbv5_to_zap_bandwidth(6000000), "BANDWIDTH_6_MHZ");
        assert_eq!(dvbv5_to_zap_bandwidth(7000000), "BANDWIDTH_7_MHZ");
        assert_eq!(dvbv5_to_zap_bandwidth(8000000), "BANDWIDTH_8_MHZ");
        assert_eq!(dvbv5_to_zap_bandwidth(5000000), "BANDWIDTH_5_MHZ");
        assert_eq!(dvbv5_to_zap_bandwidth(10000000), "BANDWIDTH_10_MHZ");
        assert_eq!(dvbv5_to_zap_bandwidth(1712000), "BANDWIDTH_1_712_MHZ");
        assert_eq!(dvbv5_to_zap_bandwidth(9999), "BANDWIDTH_AUTO");
    }

    #[test]
    fn test_dvbv5_to_zap_fec() {
        assert_eq!(dvbv5_to_zap_fec("NONE"), "FEC_NONE");
        assert_eq!(dvbv5_to_zap_fec("1/2"), "FEC_1_2");
        assert_eq!(dvbv5_to_zap_fec("2/3"), "FEC_2_3");
        assert_eq!(dvbv5_to_zap_fec("3/4"), "FEC_3_4");
        assert_eq!(dvbv5_to_zap_fec("4/5"), "FEC_4_5");
        assert_eq!(dvbv5_to_zap_fec("5/6"), "FEC_5_6");
        assert_eq!(dvbv5_to_zap_fec("6/7"), "FEC_6_7");
        assert_eq!(dvbv5_to_zap_fec("7/8"), "FEC_7_8");
        assert_eq!(dvbv5_to_zap_fec("8/9"), "FEC_8_9");
        assert_eq!(dvbv5_to_zap_fec("AUTO"), "FEC_AUTO");
        assert_eq!(dvbv5_to_zap_fec("unknown"), "FEC_AUTO");
    }

    #[test]
    fn test_dvbv5_to_zap_modulation() {
        assert_eq!(dvbv5_to_zap_modulation("QPSK"), "QPSK");
        assert_eq!(dvbv5_to_zap_modulation("QAM/16"), "QAM_16");
        assert_eq!(dvbv5_to_zap_modulation("QAM/32"), "QAM_32");
        assert_eq!(dvbv5_to_zap_modulation("QAM/64"), "QAM_64");
        assert_eq!(dvbv5_to_zap_modulation("QAM/128"), "QAM_128");
        assert_eq!(dvbv5_to_zap_modulation("QAM/256"), "QAM_256");
        assert_eq!(dvbv5_to_zap_modulation("QAM/AUTO"), "QAM_AUTO");
        assert_eq!(dvbv5_to_zap_modulation("unknown"), "QAM_AUTO");
    }

    #[test]
    fn test_dvbv5_to_zap_transmission() {
        assert_eq!(dvbv5_to_zap_transmission("2K"), "TRANSMISSION_MODE_2K");
        assert_eq!(dvbv5_to_zap_transmission("8K"), "TRANSMISSION_MODE_8K");
        assert_eq!(dvbv5_to_zap_transmission("AUTO"), "TRANSMISSION_MODE_AUTO");
        assert_eq!(dvbv5_to_zap_transmission("1K"), "TRANSMISSION_MODE_1K");
        assert_eq!(dvbv5_to_zap_transmission("4K"), "TRANSMISSION_MODE_4K");
        assert_eq!(dvbv5_to_zap_transmission("16K"), "TRANSMISSION_MODE_16K");
        assert_eq!(dvbv5_to_zap_transmission("32K"), "TRANSMISSION_MODE_32K");
        assert_eq!(dvbv5_to_zap_transmission("unknown"), "TRANSMISSION_MODE_AUTO");
    }

    #[test]
    fn test_dvbv5_to_zap_guard() {
        assert_eq!(dvbv5_to_zap_guard("1/32"), "GUARD_INTERVAL_1_32");
        assert_eq!(dvbv5_to_zap_guard("1/16"), "GUARD_INTERVAL_1_16");
        assert_eq!(dvbv5_to_zap_guard("1/8"), "GUARD_INTERVAL_1_8");
        assert_eq!(dvbv5_to_zap_guard("1/4"), "GUARD_INTERVAL_1_4");
        assert_eq!(dvbv5_to_zap_guard("AUTO"), "GUARD_INTERVAL_AUTO");
        assert_eq!(dvbv5_to_zap_guard("unknown"), "GUARD_INTERVAL_AUTO");
    }

    #[test]
    fn test_dvbv5_to_zap_hierarchy() {
        assert_eq!(dvbv5_to_zap_hierarchy("NONE"), "HIERARCHY_NONE");
        assert_eq!(dvbv5_to_zap_hierarchy("1"), "HIERARCHY_1");
        assert_eq!(dvbv5_to_zap_hierarchy("2"), "HIERARCHY_2");
        assert_eq!(dvbv5_to_zap_hierarchy("4"), "HIERARCHY_4");
        assert_eq!(dvbv5_to_zap_hierarchy("AUTO"), "HIERARCHY_AUTO");
        assert_eq!(dvbv5_to_zap_hierarchy("unknown"), "HIERARCHY_NONE");
    }

    // --- ScanEntry::to_channel ---

    #[test]
    fn test_scan_entry_to_channel() {
        let entry = ScanEntry {
            delivery_system: "DVBT".to_string(),
            frequency: 557000000,
            bandwidth_hz: 6000000,
            code_rate_hp: "2/3".to_string(),
            code_rate_lp: "AUTO".to_string(),
            modulation: "QAM/64".to_string(),
            transmission_mode: "8K".to_string(),
            guard_interval: "1/8".to_string(),
            hierarchy: "NONE".to_string(),
            inversion: "AUTO".to_string(),
        };
        let ch = entry.to_channel();
        assert_eq!(ch.frequency, 557000000);
        assert_eq!(ch.bandwidth, "BANDWIDTH_6_MHZ");
        assert_eq!(ch.fec_hp, "FEC_2_3");
        assert_eq!(ch.fec_lp, "FEC_AUTO");
        assert_eq!(ch.modulation, "QAM_64");
        assert_eq!(ch.transmission_mode, "TRANSMISSION_MODE_8K");
        assert_eq!(ch.guard_interval, "GUARD_INTERVAL_1_8");
        assert_eq!(ch.hierarchy, "HIERARCHY_NONE");
        assert_eq!(ch.inversion, "INVERSION_AUTO");
        assert_eq!(ch.video_pid, 0);
        assert_eq!(ch.audio_pid, 0);
        assert_eq!(ch.service_id, 0);
    }

    // --- parse_pat_sections ---

    #[test]
    fn test_parse_pat_sections_valid() {
        // Build a PAT section: 8-byte header + entries + 4-byte CRC
        // Each entry: 4 bytes (program_number u16 + reserved+PID u16)
        let section_length: u16 = 5 + 4 + 4 + 4; // 5 remaining header + 2 entries(8) + CRC(4) = 17
        let mut data = vec![0u8; 3 + section_length as usize];
        data[0] = 0x00; // table_id = PAT
        data[1] = 0xB0 | ((section_length >> 8) as u8 & 0x0F);
        data[2] = section_length as u8;
        // bytes 3-7: transport_stream_id, version, section_number, last_section_number
        data[3] = 0x00; data[4] = 0x01; // transport_stream_id
        data[5] = 0xC1; // version
        data[6] = 0x00; // section_number
        data[7] = 0x00; // last_section_number

        // Entry 1: program_number=0 (NIT, should be skipped), PID=0x10
        data[8] = 0x00; data[9] = 0x00;
        data[10] = 0xE0 | 0x00; data[11] = 0x10;

        // Entry 2: program_number=1, PMT PID=0x100
        data[12] = 0x00; data[13] = 0x01;
        data[14] = 0xE0 | 0x01; data[15] = 0x00;

        // CRC at end (not validated)

        let entries = parse_pat_sections(&[data]).unwrap();
        assert_eq!(entries.len(), 1); // NIT entry skipped
        assert_eq!(entries[0].service_id, 1);
        assert_eq!(entries[0].pmt_pid, 0x100);
    }

    #[test]
    fn test_parse_pat_sections_no_services() {
        // PAT with only NIT entry (program_number=0)
        let section_length: u16 = 5 + 4 + 4; // header + 1 entry + CRC
        let mut data = vec![0u8; 3 + section_length as usize];
        data[0] = 0x00;
        data[1] = 0xB0 | ((section_length >> 8) as u8 & 0x0F);
        data[2] = section_length as u8;
        data[7] = 0x00;
        // Only NIT entry
        data[8] = 0x00; data[9] = 0x00;
        data[10] = 0xE0; data[11] = 0x10;

        assert!(parse_pat_sections(&[data]).is_err());
    }

    #[test]
    fn test_parse_pat_sections_empty() {
        let result = parse_pat_sections(&[]);
        assert!(result.is_err());
    }

    // --- parse_sdt_sections ---

    #[test]
    fn test_parse_sdt_sections_empty() {
        let result = parse_sdt_sections(&[]);
        assert!(result.is_empty());
    }

    // --- parse_pmt ---

    #[test]
    fn test_parse_pmt_valid() {
        // Build a PMT section with one video (H.264) and one audio (AAC) stream
        // Header: 12 bytes + program_info_length + stream entries + CRC
        let program_info_length: u16 = 0;
        let stream1_es_info_len: u16 = 0; // video stream
        let stream2_es_info_len: u16 = 0; // audio stream
        let entries_size = 2 * 5; // 2 streams * 5 bytes each (no ES info)
        let section_length: u16 = 9 + program_info_length + entries_size as u16 + 4; // 9 remaining header + entries + CRC

        let mut data = vec![0u8; 3 + section_length as usize];
        data[0] = 0x02; // table_id = PMT
        data[1] = 0xB0 | ((section_length >> 8) as u8 & 0x0F);
        data[2] = section_length as u8;
        data[3] = 0x00; data[4] = 0x01; // program_number
        data[5] = 0xC1; // version
        data[6] = 0x00; data[7] = 0x00; // section numbers
        data[8] = 0xE0; data[9] = 0x00; // PCR PID
        data[10] = 0xF0 | ((program_info_length >> 8) as u8 & 0x0F);
        data[11] = program_info_length as u8;

        // Stream 1: H.264 video, PID=0x100
        let pos = 12;
        data[pos] = 0x1B; // stream_type = H.264
        data[pos + 1] = 0xE0 | 0x01; data[pos + 2] = 0x00; // PID = 0x100
        data[pos + 3] = 0xF0 | ((stream1_es_info_len >> 8) as u8 & 0x0F);
        data[pos + 4] = stream1_es_info_len as u8;

        // Stream 2: AAC audio, PID=0x101
        let pos = 17;
        data[pos] = 0x0F; // stream_type = AAC
        data[pos + 1] = 0xE0 | 0x01; data[pos + 2] = 0x01; // PID = 0x101
        data[pos + 3] = 0xF0 | ((stream2_es_info_len >> 8) as u8 & 0x0F);
        data[pos + 4] = stream2_es_info_len as u8;

        let pmt = parse_pmt(&data).unwrap();
        assert_eq!(pmt.video_pid, 0x100);
        assert_eq!(pmt.audio_pid, 0x101);
    }

    #[test]
    fn test_parse_pmt_too_short() {
        let data = [0u8; 15];
        assert!(parse_pmt(&data).is_err());
    }

    #[test]
    fn test_parse_pmt_no_streams() {
        // PMT with no elementary streams
        let section_length: u16 = 9 + 4; // header + CRC only
        let mut data = vec![0u8; 3 + section_length as usize];
        data[0] = 0x02;
        data[1] = 0xB0 | ((section_length >> 8) as u8 & 0x0F);
        data[2] = section_length as u8;
        data[10] = 0xF0; data[11] = 0x00; // program_info_length = 0

        let pmt = parse_pmt(&data).unwrap();
        assert_eq!(pmt.video_pid, 0);
        assert_eq!(pmt.audio_pid, 0);
    }
}

// --- Generic section reader ---

/// Read all sections for a given PID/table_id, collecting until we have
/// section_number 0 through last_section_number. Returns all raw section buffers.
fn read_all_sections(adapter: u32, pid: u16, table_id: u8, timeout_secs: u64) -> Result<Vec<Vec<u8>>, String> {
    let mut demux_file = dmx::open_demux_with_filter(adapter, pid)?;
    let fd = demux_file.as_raw_fd();

    let mut buf = [0u8; 4096];
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let mut sections: std::collections::HashMap<u8, Vec<u8>> = std::collections::HashMap::new();
    let mut expected_last: Option<u8> = None;

    while start.elapsed() < timeout {
        let remaining_ms = timeout
            .checked_sub(start.elapsed())
            .unwrap_or_default()
            .as_millis() as i32;
        if remaining_ms <= 0 {
            break;
        }

        let poll_ms = remaining_ms.min(5000);
        let poll_fd = PollFd::new(
            unsafe { std::os::unix::io::BorrowedFd::borrow_raw(fd) },
            PollFlags::POLLIN,
        );
        let poll_timeout = PollTimeout::try_from(poll_ms).unwrap_or(PollTimeout::NONE);
        let nfds =
            poll(&mut [poll_fd], poll_timeout).map_err(|e| format!("poll failed: {e}"))?;

        if nfds == 0 {
            continue;
        }

        let n = match demux_file.read(&mut buf) {
            Ok(n) => n,
            Err(_) => continue,
        };

        if n < 8 {
            continue;
        }

        // Filter table_id in userspace
        if buf[0] != table_id {
            continue;
        }

        let section_number = buf[6];
        let last_section_number = buf[7];

        expected_last = Some(last_section_number);
        sections.entry(section_number).or_insert_with(|| buf[..n].to_vec());

        // Check if we have all sections
        let last = last_section_number as usize;
        if sections.len() > last {
            break;
        }
    }

    if sections.is_empty() {
        return Err(format!(
            "Timeout reading sections (PID=0x{pid:04X}, table_id=0x{table_id:02X})"
        ));
    }

    // Return sections sorted by section_number
    let mut result: Vec<(u8, Vec<u8>)> = sections.into_iter().collect();
    result.sort_by_key(|(num, _)| *num);

    if let Some(last) = expected_last {
        if result.len() <= last as usize {
            eprintln!(
                "  Warning: only got {}/{} sections for PID=0x{pid:04X}",
                result.len(),
                last + 1
            );
        }
    }

    Ok(result.into_iter().map(|(_, data)| data).collect())
}

// --- PAT parsing (PID 0x0000, table_id 0x00) ---

struct PatEntry {
    service_id: u16,
    pmt_pid: u16,
}

fn parse_pat_sections(sections: &[Vec<u8>]) -> Result<Vec<PatEntry>, String> {
    let mut entries = Vec::new();

    for data in sections {
        if data.len() < 12 {
            continue;
        }

        let section_length = (((data[1] & 0x0F) as usize) << 8) | data[2] as usize;
        let section_end = 3 + section_length;
        if data.len() < section_end {
            continue;
        }

        let entries_end = section_end - 4; // exclude CRC
        let mut pos = 8; // after 8-byte header

        while pos + 4 <= entries_end {
            let program_number = u16::from_be_bytes([data[pos], data[pos + 1]]);
            let pid = ((data[pos + 2] & 0x1F) as u16) << 8 | data[pos + 3] as u16;

            if program_number != 0 {
                // Skip NIT entry (program_number 0)
                entries.push(PatEntry {
                    service_id: program_number,
                    pmt_pid: pid,
                });
            }
            pos += 4;
        }
    }

    if entries.is_empty() {
        return Err("PAT: no services found".to_string());
    }

    Ok(entries)
}

// --- SDT parsing (PID 0x0011, table_id 0x42) ---

fn parse_sdt_sections(sections: &[Vec<u8>]) -> Vec<(u16, String)> {
    let mut services = Vec::new();

    for data in sections {
        if data.len() < 15 {
            continue;
        }

        let section_length = (((data[1] & 0x0F) as usize) << 8) | data[2] as usize;
        let section_end = 3 + section_length;
        if data.len() < section_end {
            continue;
        }

        let entries_end = section_end - 4;
        let mut pos = 11; // after 11-byte SDT header

        while pos + 5 <= entries_end {
            let service_id = u16::from_be_bytes([data[pos], data[pos + 1]]);
            let desc_loop_length =
                (((data[pos + 3] & 0x0F) as usize) << 8) | data[pos + 4] as usize;
            pos += 5;

            if pos + desc_loop_length > entries_end {
                break;
            }

            let desc_end = pos + desc_loop_length;
            let mut service_name = String::new();
            let mut dpos = pos;

            while dpos + 2 <= desc_end {
                let tag = data[dpos];
                let len = data[dpos + 1] as usize;
                if dpos + 2 + len > desc_end {
                    break;
                }

                // Service descriptor (tag 0x48)
                if tag == 0x48 && len >= 2 {
                    let desc = &data[dpos + 2..dpos + 2 + len];
                    // desc[0] = service_type
                    let provider_len = desc[1] as usize;
                    if 2 + provider_len + 1 <= desc.len() {
                        let name_len = desc[2 + provider_len] as usize;
                        if 3 + provider_len + name_len <= desc.len() {
                            service_name = decode_dvb_text(
                                &desc[3 + provider_len..3 + provider_len + name_len],
                            );
                        }
                    }
                }

                dpos += 2 + len;
            }

            services.push((service_id, service_name));
            pos = desc_end;
        }
    }

    services
}

// --- PMT parsing (variable PID, table_id 0x02) ---

struct PmtInfo {
    video_pid: u16,
    audio_pid: u16,
}

fn parse_pmt(data: &[u8]) -> Result<PmtInfo, String> {
    if data.len() < 16 {
        return Err("PMT too short".to_string());
    }

    let section_length = (((data[1] & 0x0F) as usize) << 8) | data[2] as usize;
    let section_end = 3 + section_length;
    if data.len() < section_end {
        return Err("PMT truncated".to_string());
    }

    let program_info_length = (((data[10] & 0x0F) as usize) << 8) | data[11] as usize;
    let entries_end = section_end - 4;
    let mut pos = 12 + program_info_length;

    let mut video_pid: u16 = 0;
    let mut audio_pid: u16 = 0;

    while pos + 5 <= entries_end {
        let stream_type = data[pos];
        let elementary_pid = ((data[pos + 1] & 0x1F) as u16) << 8 | data[pos + 2] as u16;
        let es_info_length = (((data[pos + 3] & 0x0F) as usize) << 8) | data[pos + 4] as usize;

        // Video: MPEG-1(0x01), MPEG-2(0x02), MPEG-4(0x10), H.264(0x1B), H.265(0x24)
        if video_pid == 0 && matches!(stream_type, 0x01 | 0x02 | 0x10 | 0x1B | 0x24) {
            video_pid = elementary_pid;
        }
        // Audio: MPEG-1(0x03), MPEG-2(0x04), AAC(0x0F), HE-AAC(0x11)
        if audio_pid == 0 && matches!(stream_type, 0x03 | 0x04 | 0x0F | 0x11) {
            audio_pid = elementary_pid;
        }

        pos += 5 + es_info_length;
    }

    Ok(PmtInfo {
        video_pid,
        audio_pid,
    })
}

// --- Channel scanning orchestrator ---

/// After tuning to a frequency, scan PAT/SDT/PMT to discover services.
/// Returns a list of Channel entries with full tuning params and discovered PIDs/names.
pub fn scan_frequency(adapter: u32, entry: &ScanEntry) -> Result<Vec<Channel>, String> {
    let base = entry.to_channel();

    // Read PAT (PID 0x0000, table_id 0x00) - collect all sections
    let pat_sections = read_all_sections(adapter, 0x0000, 0x00, 5)?;
    let pat_entries = parse_pat_sections(&pat_sections)?;

    // Read SDT (PID 0x0011, table_id 0x42) - collect all sections
    let sdt_services = match read_all_sections(adapter, 0x0011, 0x42, 5) {
        Ok(sdt_sections) => parse_sdt_sections(&sdt_sections),
        Err(_) => Vec::new(),
    };

    let mut channels = Vec::new();

    for pat_entry in &pat_entries {
        // Look up service name from SDT
        let name = sdt_services
            .iter()
            .find(|(sid, _)| *sid == pat_entry.service_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| format!("Service {}", pat_entry.service_id));

        // Read PMT for this service (single section per program)
        let pmt = match read_all_sections(adapter, pat_entry.pmt_pid, 0x02, 5) {
            Ok(sections) if !sections.is_empty() => {
                parse_pmt(&sections[0]).unwrap_or(PmtInfo {
                    video_pid: 0,
                    audio_pid: 0,
                })
            }
            _ => PmtInfo {
                video_pid: 0,
                audio_pid: 0,
            },
        };

        channels.push(Channel {
            name,
            frequency: base.frequency,
            inversion: base.inversion.clone(),
            bandwidth: base.bandwidth.clone(),
            fec_hp: base.fec_hp.clone(),
            fec_lp: base.fec_lp.clone(),
            modulation: base.modulation.clone(),
            transmission_mode: base.transmission_mode.clone(),
            guard_interval: base.guard_interval.clone(),
            hierarchy: base.hierarchy.clone(),
            video_pid: pmt.video_pid,
            audio_pid: pmt.audio_pid,
            service_id: pat_entry.service_id,
        });
    }

    Ok(channels)
}
