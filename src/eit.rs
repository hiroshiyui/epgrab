use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::time::Instant;

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

use crate::dmx;

// EIT PID
const EIT_PID: u16 = 0x12;

// EIT table IDs
const EIT_PRESENT_FOLLOWING_ACTUAL: u8 = 0x4E;
const EIT_SCHEDULE_ACTUAL_MIN: u8 = 0x50;
const EIT_SCHEDULE_ACTUAL_MAX: u8 = 0x5F;

// Short event descriptor tag
const SHORT_EVENT_DESCRIPTOR: u8 = 0x4D;

#[allow(dead_code)]
pub struct EitEvent {
    pub service_id: u16,
    pub event_id: u16,
    pub start_time: i64,
    pub duration: u32,
    pub running_status: u8,
    pub event_name: String,
    pub description: String,
    pub language: String,
}

fn bcd_to_u8(bcd: u8) -> u8 {
    ((bcd >> 4) * 10) + (bcd & 0x0F)
}

fn decode_start_time(data: &[u8]) -> i64 {
    // data[0..2]: MJD (big-endian u16)
    // data[2]: hours BCD
    // data[3]: minutes BCD
    // data[4]: seconds BCD
    let mjd = u16::from_be_bytes([data[0], data[1]]);
    let epoch = (mjd as i64 - 40587) * 86400;
    epoch
        + bcd_to_u8(data[2]) as i64 * 3600
        + bcd_to_u8(data[3]) as i64 * 60
        + bcd_to_u8(data[4]) as i64
}

fn decode_duration(data: &[u8]) -> u32 {
    bcd_to_u8(data[0]) as u32 * 3600
        + bcd_to_u8(data[1]) as u32 * 60
        + bcd_to_u8(data[2]) as u32
}

pub fn decode_dvb_text(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    let mut raw = match data[0] {
        0x14 => {
            // Big5 subset of ISO/IEC 10646: UTF-16 BE
            if data.len() < 3 {
                return String::new();
            }
            let pairs: Vec<u16> = data[1..]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&pairs)
        }
        0x15 => {
            // UTF-8
            String::from_utf8_lossy(&data[1..]).to_string()
        }
        0x11 => {
            // ISO/IEC 10646 BMP (UCS-2)
            if data.len() < 3 {
                return String::new();
            }
            let pairs: Vec<u16> = data[1..]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&pairs)
        }
        0x10 => {
            // ISO 8859-N: skip 3-byte prefix
            if data.len() > 3 {
                String::from_utf8_lossy(&data[3..]).to_string()
            } else {
                String::new()
            }
        }
        0x01..=0x05 => {
            // ISO 8859 tables: skip prefix byte
            String::from_utf8_lossy(&data[1..]).to_string()
        }
        0x20..=0xFF => {
            // Default table (ISO 6937), treat as best-effort
            String::from_utf8_lossy(data).to_string()
        }
        _ => String::new(),
    };
    // Strip DVB control characters and other non-printable characters:
    // U+0000-U+001F: C0 controls (except U+000A newline)
    // U+007F: DELETE
    // U+0080-U+009F: C1 controls (DVB emphasis on/off 0x86/0x87, line break 0x8A, etc.)
    // U+E080-U+E09F: some DVB implementations map control codes to Private Use Area
    raw.retain(|c| {
        let cp = c as u32;
        if cp == 0x0A {
            return true; // keep newline
        }
        if cp <= 0x1F || cp == 0x7F || (0x80..=0x9F).contains(&cp) {
            return false;
        }
        if (0xE080..=0xE09F).contains(&cp) {
            return false;
        }
        true
    });
    raw
}

fn parse_short_event_descriptor(data: &[u8]) -> (String, String, String) {
    // language: 3 bytes
    // event_name_length: 1 byte
    // event_name: N bytes
    // text_length: 1 byte
    // text: M bytes
    if data.len() < 5 {
        return (String::new(), String::new(), String::new());
    }

    let language = String::from_utf8_lossy(&data[0..3]).to_string();
    let name_len = data[3] as usize;

    if data.len() < 4 + name_len + 1 {
        return (language, String::new(), String::new());
    }

    let event_name = decode_dvb_text(&data[4..4 + name_len]);

    let text_offset = 4 + name_len;
    let text_len = data[text_offset] as usize;

    let description = if data.len() >= text_offset + 1 + text_len {
        decode_dvb_text(&data[text_offset + 1..text_offset + 1 + text_len])
    } else {
        String::new()
    };

    (language, event_name, description)
}

fn parse_eit_event(data: &[u8], service_id: u16) -> Result<(EitEvent, usize), String> {
    if data.len() < 12 {
        return Err("Event data too short".to_string());
    }

    let event_id = u16::from_be_bytes([data[0], data[1]]);
    let start_time = decode_start_time(&data[2..7]);
    let duration = decode_duration(&data[7..10]);
    let running_status = (data[10] >> 5) & 0x07;
    let descriptors_length = (((data[10] & 0x0F) as usize) << 8) | data[11] as usize;

    let total_len = 12 + descriptors_length;
    if total_len > data.len() {
        return Err(format!(
            "Event data truncated: need {total_len}, have {}",
            data.len()
        ));
    }

    // Sanity check: reject events with unreasonable values
    if duration > 86400 || start_time < 0 {
        return Err("Event has unreasonable duration or start time".to_string());
    }

    // Parse descriptors
    let mut language = String::new();
    let mut event_name = String::new();
    let mut description = String::new();

    let desc_data = &data[12..12 + descriptors_length];
    let mut pos = 0;
    while pos + 2 <= desc_data.len() {
        let tag = desc_data[pos];
        let len = desc_data[pos + 1] as usize;
        if pos + 2 + len > desc_data.len() {
            break;
        }
        if tag == SHORT_EVENT_DESCRIPTOR {
            let desc_bytes = &desc_data[pos + 2..pos + 2 + len];
            let (lang, name, desc) = parse_short_event_descriptor(desc_bytes);
            language = lang;
            event_name = name;
            description = desc;
        }
        pos += 2 + len;
    }

    Ok((
        EitEvent {
            service_id,
            event_id,
            start_time,
            duration,
            running_status,
            event_name,
            description,
            language,
        },
        total_len,
    ))
}

fn parse_eit_section(buf: &[u8]) -> Result<(u16, Vec<EitEvent>), String> {
    // Minimum: 14 header + 4 CRC = 18 bytes
    if buf.len() < 18 {
        return Err("Section too short".to_string());
    }

    let table_id = buf[0];
    let section_length = (((buf[1] & 0x0F) as usize) << 8) | buf[2] as usize;
    let service_id = u16::from_be_bytes([buf[3], buf[4]]);

    // section body starts after the 3-byte header (table_id + section_length)
    // event data starts at offset 14
    // CRC is last 4 bytes of section
    let section_end = 3 + section_length;
    if buf.len() < section_end {
        return Err(format!(
            "Section truncated: need {section_end}, have {}",
            buf.len()
        ));
    }

    let events_end = section_end - 4; // exclude CRC
    let mut events = Vec::new();
    let mut pos = 14; // skip 14-byte header

    // Sanity check: for table 0x4E, last_section_number should be 0 or 1
    let last_section_number = buf[7];
    if table_id == 0x4E && last_section_number > 1 {
        return Err(format!(
            "Corrupted section: table 0x4E has last_section_number={last_section_number} (expected 0 or 1)"
        ));
    }

    while pos < events_end {
        match parse_eit_event(&buf[pos..events_end], service_id) {
            Ok((event, consumed)) => {
                events.push(event);
                pos += consumed;
            }
            Err(e) => {
                eprintln!("Warning: failed to parse EIT event at offset {pos}: {e}");
                break;
            }
        }
    }

    Ok((service_id, events))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- bcd_to_u8 ---

    #[test]
    fn test_bcd_to_u8_zero() {
        assert_eq!(bcd_to_u8(0x00), 0);
    }

    #[test]
    fn test_bcd_to_u8_single_digit() {
        assert_eq!(bcd_to_u8(0x09), 9);
    }

    #[test]
    fn test_bcd_to_u8_double_digit() {
        assert_eq!(bcd_to_u8(0x23), 23);
        assert_eq!(bcd_to_u8(0x59), 59);
        assert_eq!(bcd_to_u8(0x99), 99);
    }

    // --- decode_start_time ---

    #[test]
    fn test_decode_start_time_epoch() {
        // MJD 40587 = Unix epoch (1970-01-01), time 00:00:00
        let data = [0x9E, 0x8B, 0x00, 0x00, 0x00]; // MJD=40587
        assert_eq!(decode_start_time(&data), 0);
    }

    #[test]
    fn test_decode_start_time_known_date() {
        // 2025-01-15 14:30:00 UTC
        // MJD for 2025-01-15 = 60690
        // 60690 in big-endian = 0xED12
        let mjd: u16 = 60690;
        let bytes = mjd.to_be_bytes();
        let data = [bytes[0], bytes[1], 0x14, 0x30, 0x00]; // 14:30:00 BCD
        let expected = (60690i64 - 40587) * 86400 + 14 * 3600 + 30 * 60;
        assert_eq!(decode_start_time(&data), expected);
    }

    #[test]
    fn test_decode_start_time_midnight() {
        let mjd: u16 = 51544; // 2000-01-01
        let bytes = mjd.to_be_bytes();
        let data = [bytes[0], bytes[1], 0x00, 0x00, 0x00];
        let expected = (51544i64 - 40587) * 86400;
        assert_eq!(decode_start_time(&data), expected);
    }

    #[test]
    fn test_decode_start_time_end_of_day() {
        let mjd: u16 = 51544;
        let bytes = mjd.to_be_bytes();
        let data = [bytes[0], bytes[1], 0x23, 0x59, 0x59]; // 23:59:59 BCD
        let expected = (51544i64 - 40587) * 86400 + 23 * 3600 + 59 * 60 + 59;
        assert_eq!(decode_start_time(&data), expected);
    }

    // --- decode_duration ---

    #[test]
    fn test_decode_duration_zero() {
        assert_eq!(decode_duration(&[0x00, 0x00, 0x00]), 0);
    }

    #[test]
    fn test_decode_duration_one_hour() {
        assert_eq!(decode_duration(&[0x01, 0x00, 0x00]), 3600);
    }

    #[test]
    fn test_decode_duration_half_hour() {
        assert_eq!(decode_duration(&[0x00, 0x30, 0x00]), 1800);
    }

    #[test]
    fn test_decode_duration_complex() {
        // 2h 45m 30s
        assert_eq!(decode_duration(&[0x02, 0x45, 0x30]), 2 * 3600 + 45 * 60 + 30);
    }

    // --- decode_dvb_text ---

    #[test]
    fn test_decode_dvb_text_empty() {
        assert_eq!(decode_dvb_text(&[]), "");
    }

    #[test]
    fn test_decode_dvb_text_utf8() {
        // 0x15 prefix = UTF-8
        let data = [0x15, b'H', b'e', b'l', b'l', b'o'];
        assert_eq!(decode_dvb_text(&data), "Hello");
    }

    #[test]
    fn test_decode_dvb_text_utf8_with_cjk() {
        // 0x15 prefix = UTF-8, followed by "テスト" in UTF-8
        let mut data = vec![0x15];
        data.extend_from_slice("テスト".as_bytes());
        assert_eq!(decode_dvb_text(&data), "テスト");
    }

    #[test]
    fn test_decode_dvb_text_default_table() {
        // Bytes 0x20..=0xFF use default table (ISO 6937)
        let data = b"Hello World";
        assert_eq!(decode_dvb_text(data), "Hello World");
    }

    #[test]
    fn test_decode_dvb_text_strips_control_chars() {
        // UTF-16 BE (0x11 prefix) with C1 control chars embedded
        // U+0086 (emphasis on) and U+0087 (emphasis off) as UTF-16 BE
        let data = [
            0x11, // UCS-2
            0x00, 0x41, // 'A'
            0x00, 0x86, // U+0086 (should be stripped)
            0x00, 0x42, // 'B'
            0x00, 0x87, // U+0087 (should be stripped)
            0x00, 0x43, // 'C'
        ];
        assert_eq!(decode_dvb_text(&data), "ABC");
    }

    #[test]
    fn test_decode_dvb_text_strips_c0_controls() {
        // Default encoding with C0 control chars (0x01-0x1F except 0x0A)
        // 0x15 prefix (UTF-8), then text with tab (0x09) which should be stripped
        let data = [0x15, b'A', 0x09, b'B'];
        assert_eq!(decode_dvb_text(&data), "AB");
    }

    #[test]
    fn test_decode_dvb_text_keeps_newline() {
        let data = [0x15, b'A', 0x0A, b'B'];
        assert_eq!(decode_dvb_text(&data), "A\nB");
    }

    #[test]
    fn test_decode_dvb_text_utf16_be() {
        // 0x14 = Big5 subset / UTF-16 BE
        // "Hi" in UTF-16 BE: 0x00 0x48 0x00 0x69
        let data = [0x14, 0x00, 0x48, 0x00, 0x69];
        assert_eq!(decode_dvb_text(&data), "Hi");
    }

    #[test]
    fn test_decode_dvb_text_utf16_too_short() {
        let data = [0x14, 0x00]; // only 2 bytes total, too short
        assert_eq!(decode_dvb_text(&data), "");
    }

    #[test]
    fn test_decode_dvb_text_iso8859_prefix() {
        // 0x01..=0x05 skip one byte prefix
        let data = [0x01, b'T', b'e', b's', b't'];
        assert_eq!(decode_dvb_text(&data), "Test");
    }

    #[test]
    fn test_decode_dvb_text_iso8859_n() {
        // 0x10 + 2 more bytes = 3-byte prefix for ISO 8859-N
        let data = [0x10, 0x00, 0x01, b'A', b'B', b'C'];
        assert_eq!(decode_dvb_text(&data), "ABC");
    }

    #[test]
    fn test_decode_dvb_text_iso8859_n_too_short() {
        let data = [0x10, 0x00, 0x01]; // exactly 3 bytes, no text
        assert_eq!(decode_dvb_text(&data), "");
    }

    #[test]
    fn test_decode_dvb_text_ucs2() {
        // 0x11 = UCS-2 (ISO/IEC 10646 BMP)
        let data = [0x11, 0x00, 0x41, 0x00, 0x42]; // "AB"
        assert_eq!(decode_dvb_text(&data), "AB");
    }

    #[test]
    fn test_decode_dvb_text_unknown_prefix() {
        // 0x06..=0x0F are unhandled → empty
        assert_eq!(decode_dvb_text(&[0x06]), "");
        assert_eq!(decode_dvb_text(&[0x0F]), "");
    }

    // --- parse_short_event_descriptor ---

    #[test]
    fn test_parse_short_event_descriptor_valid() {
        // language: "eng", name_len: 5, name: "Hello" (default encoding), text_len: 5, text: "World"
        let data = [
            b'e', b'n', b'g', // language
            5,                 // event_name_length
            b'H', b'e', b'l', b'l', b'o', // event_name (default encoding, 0x20+)
            5,                 // text_length
            b'W', b'o', b'r', b'l', b'd', // text
        ];
        let (lang, name, desc) = parse_short_event_descriptor(&data);
        assert_eq!(lang, "eng");
        assert_eq!(name, "Hello");
        assert_eq!(desc, "World");
    }

    #[test]
    fn test_parse_short_event_descriptor_too_short() {
        let data = [b'e', b'n', b'g', 0]; // only 4 bytes, need at least 5
        let (lang, name, desc) = parse_short_event_descriptor(&data);
        assert_eq!(lang, "");
        assert_eq!(name, "");
        assert_eq!(desc, "");
    }

    #[test]
    fn test_parse_short_event_descriptor_empty_fields() {
        let data = [b'z', b'h', b'o', 0, 0]; // zero-length name and text
        let (lang, name, desc) = parse_short_event_descriptor(&data);
        assert_eq!(lang, "zho");
        assert_eq!(name, "");
        assert_eq!(desc, "");
    }

    #[test]
    fn test_parse_short_event_descriptor_name_only() {
        let data = [
            b'j', b'p', b'n', // language
            3,                 // name_len
            b'A', b'B', b'C', // name
            0,                 // text_len = 0
        ];
        let (lang, name, desc) = parse_short_event_descriptor(&data);
        assert_eq!(lang, "jpn");
        assert_eq!(name, "ABC");
        assert_eq!(desc, "");
    }

    // --- parse_eit_event ---

    #[test]
    fn test_parse_eit_event_minimal() {
        // Build a minimal EIT event: 12-byte header + 0 bytes descriptors
        let mjd: u16 = 51544; // 2000-01-01
        let mjd_bytes = mjd.to_be_bytes();
        let data = [
            0x00, 0x01, // event_id = 1
            mjd_bytes[0], mjd_bytes[1], 0x12, 0x00, 0x00, // start_time: 12:00:00
            0x01, 0x30, 0x00, // duration: 1h30m00s BCD
            0x00, 0x00, // running_status=0, descriptors_length=0
        ];
        let (event, consumed) = parse_eit_event(&data, 100).unwrap();
        assert_eq!(event.event_id, 1);
        assert_eq!(event.service_id, 100);
        assert_eq!(event.duration, 5400); // 1h30m
        assert_eq!(consumed, 12);
    }

    #[test]
    fn test_parse_eit_event_too_short() {
        let data = [0u8; 11]; // less than 12 bytes
        assert!(parse_eit_event(&data, 1).is_err());
    }

    #[test]
    fn test_parse_eit_event_with_descriptor() {
        let mjd: u16 = 51544;
        let mjd_bytes = mjd.to_be_bytes();

        // Build short event descriptor content:
        // language "eng" + name_len 4 + "Test" + text_len 4 + "Desc"
        let descriptor_content = [
            b'e', b'n', b'g', // language
            4,                 // name_len
            b'T', b'e', b's', b't', // name
            4,                 // text_len
            b'D', b'e', b's', b'c', // text
        ];
        let desc_len = descriptor_content.len();

        // Full descriptor: tag(1) + len(1) + content
        let full_desc_len = 2 + desc_len;

        let mut data = vec![
            0x00, 0x42, // event_id = 0x42
            mjd_bytes[0], mjd_bytes[1], 0x10, 0x00, 0x00, // start_time: 10:00:00
            0x00, 0x30, 0x00, // duration: 30m
        ];
        // running_status=4 (running), descriptors_length
        let rs_byte = (4u8 << 5) | ((full_desc_len >> 8) as u8 & 0x0F);
        data.push(rs_byte);
        data.push(full_desc_len as u8);
        // Descriptor tag + length + content
        data.push(SHORT_EVENT_DESCRIPTOR);
        data.push(desc_len as u8);
        data.extend_from_slice(&descriptor_content);

        let (event, consumed) = parse_eit_event(&data, 200).unwrap();
        assert_eq!(event.event_id, 0x42);
        assert_eq!(event.service_id, 200);
        assert_eq!(event.running_status, 4);
        assert_eq!(event.duration, 1800);
        assert_eq!(event.event_name, "Test");
        assert_eq!(event.description, "Desc");
        assert_eq!(event.language, "eng");
        assert_eq!(consumed, 12 + full_desc_len);
    }

    #[test]
    fn test_parse_eit_event_rejects_unreasonable_duration() {
        let mjd: u16 = 51544;
        let mjd_bytes = mjd.to_be_bytes();
        let data = [
            0x00, 0x01,
            mjd_bytes[0], mjd_bytes[1], 0x12, 0x00, 0x00,
            0x25, 0x00, 0x00, // 25 hours in BCD = 90000s > 86400
            0x00, 0x00,
        ];
        assert!(parse_eit_event(&data, 1).is_err());
    }

    // --- parse_eit_section ---

    #[test]
    fn test_parse_eit_section_too_short() {
        let data = [0u8; 17]; // less than 18 bytes
        assert!(parse_eit_section(&data).is_err());
    }

    #[test]
    fn test_parse_eit_section_minimal() {
        // Build a minimal valid EIT section with no events
        // table_id(1) + section_length(2, points to 15 bytes: 11 header remaining + 4 CRC)
        // section_length = 15 means 3+15=18 total
        let mut data = vec![0u8; 18];
        data[0] = EIT_PRESENT_FOLLOWING_ACTUAL; // table_id = 0x4E
        // section_length = 15 (0x000F)
        data[1] = 0xF0 | 0x00; // section_syntax_indicator + reserved + length high
        data[2] = 15;          // length low
        data[3] = 0x00; data[4] = 0x01; // service_id = 1
        data[5] = 0xC1; // version, current_next
        data[6] = 0x00; // section_number = 0
        data[7] = 0x00; // last_section_number = 0
        // bytes 8-13: transport_stream_id, original_network_id, etc.
        // CRC at bytes 14-17

        let (service_id, events) = parse_eit_section(&data).unwrap();
        assert_eq!(service_id, 1);
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_eit_section_corrupted_table_0x4e() {
        // table 0x4E with last_section_number > 1 should be rejected
        let mut data = vec![0u8; 18];
        data[0] = 0x4E;
        data[1] = 0xF0;
        data[2] = 15;
        data[3] = 0x00; data[4] = 0x01;
        data[7] = 5; // last_section_number = 5, invalid for 0x4E
        assert!(parse_eit_section(&data).is_err());
    }
}

pub struct EitReader {
    demux_file: std::fs::File,
}

impl EitReader {
    /// Open the demux device and set up the EIT section filter.
    pub fn open(adapter: u32) -> Result<Self, String> {
        let demux_file = dmx::open_demux_with_filter(adapter, EIT_PID)?;
        Ok(EitReader { demux_file })
    }

    /// Read EIT sections for the given timeout duration.
    pub fn read_events(&mut self, timeout_secs: u64) -> Result<Vec<EitEvent>, String> {
        let fd = self.demux_file.as_raw_fd();
        let mut all_events = Vec::new();
        let mut section_buf = [0u8; 4096];
        let start = Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);
        let mut seen_sections: std::collections::HashSet<(u16, u8, u8)> = std::collections::HashSet::new();
        let mut seen_events: std::collections::HashSet<(u16, u16)> = std::collections::HashSet::new();

        while start.elapsed() < timeout {
            let remaining_ms = timeout
                .checked_sub(start.elapsed())
                .unwrap_or_default()
                .as_millis() as i32;

            if remaining_ms <= 0 {
                break;
            }

            // Use short poll intervals (5s) to avoid blocking too long
            let poll_ms = remaining_ms.min(5000);
            let poll_fd = PollFd::new(
                unsafe { std::os::unix::io::BorrowedFd::borrow_raw(fd) },
                PollFlags::POLLIN,
            );
            let poll_timeout = PollTimeout::try_from(poll_ms)
                .unwrap_or(PollTimeout::NONE);
            let nfds = poll(&mut [poll_fd], poll_timeout)
                .map_err(|e| format!("poll failed: {e}"))?;

            if nfds == 0 {
                continue; // keep trying until overall timeout
            }

            let n = match self.demux_file.read(&mut section_buf) {
                Ok(n) => n,
                Err(_) => continue,
            };

            if n < 18 {
                continue;
            }

            // Filter for EIT present/following (0x4E) and schedule (0x50-0x5F)
            let table_id = section_buf[0];
            let is_pf = table_id == EIT_PRESENT_FOLLOWING_ACTUAL;
            let is_sched = table_id >= EIT_SCHEDULE_ACTUAL_MIN
                && table_id <= EIT_SCHEDULE_ACTUAL_MAX;
            if !is_pf && !is_sched {
                continue;
            }

            // Track sections to avoid duplicates
            let section_number = section_buf[6];
            let service_id = u16::from_be_bytes([section_buf[3], section_buf[4]]);
            let key = (service_id, table_id, section_number);

            if seen_sections.contains(&key) {
                continue;
            }
            seen_sections.insert(key);

            match parse_eit_section(&section_buf[..n]) {
                Ok((_sid, events)) => {
                    for event in events {
                        let event_key = (event.service_id, event.event_id);
                        if seen_events.insert(event_key) {
                            all_events.push(event);
                        }
                    }
                }
                Err(_) => {}
            }
        }

        all_events.sort_by_key(|e| e.start_time);
        Ok(all_events)
    }
}
