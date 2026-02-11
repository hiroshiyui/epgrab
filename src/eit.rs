use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::time::Instant;

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

// Demux constants
const DMX_FILTER_SIZE: usize = 16;
#[allow(dead_code)]
const DMX_CHECK_CRC: u32 = 1;
const DMX_IMMEDIATE_START: u32 = 4;

// EIT PID
const EIT_PID: u16 = 0x12;

// EIT table IDs
const EIT_PRESENT_FOLLOWING_ACTUAL: u8 = 0x4E;

// Short event descriptor tag
const SHORT_EVENT_DESCRIPTOR: u8 = 0x4D;

// Demux filter structs matching kernel layout
#[repr(C)]
struct DmxFilter {
    filter: [u8; DMX_FILTER_SIZE],
    mask: [u8; DMX_FILTER_SIZE],
    mode: [u8; DMX_FILTER_SIZE],
}

#[repr(C)]
struct DmxSctFilterParams {
    pid: u16,
    filter: DmxFilter,
    timeout: u32,
    flags: u32,
}

nix::ioctl_write_ptr!(dmx_set_filter, b'o', 43, DmxSctFilterParams);

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
    match data[0] {
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
    }
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

    let _table_id = buf[0];
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
    if _table_id == 0x4E && last_section_number > 1 {
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

pub struct EitReader {
    demux_file: std::fs::File,
}

impl EitReader {
    /// Open the demux device and set up the EIT section filter.
    /// Call this BEFORE tuning the frontend.
    pub fn open(adapter: u32) -> Result<Self, String> {
        let path = format!("/dev/dvb/adapter{adapter}/demux0");
        let demux_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("Failed to open {path}: {e}"))?;

        let fd = demux_file.as_raw_fd();

        // Set up section filter for EIT PID 0x12 (filter table_id in userspace
        // since some drivers don't support section header filtering correctly)
        let params = DmxSctFilterParams {
            pid: EIT_PID,
            filter: DmxFilter {
                filter: [0u8; DMX_FILTER_SIZE],
                mask: [0u8; DMX_FILTER_SIZE],
                mode: [0u8; DMX_FILTER_SIZE],
            },
            timeout: 0,
            flags: DMX_IMMEDIATE_START,
        };

        unsafe {
            dmx_set_filter(fd, &params).map_err(|e| format!("DMX_SET_FILTER failed: {e}"))?;
        }

        Ok(EitReader { demux_file })
    }

    /// Read EIT sections for the given timeout duration.
    pub fn read_events(&mut self, timeout_secs: u64) -> Result<Vec<EitEvent>, String> {
        let fd = self.demux_file.as_raw_fd();
        let mut all_events = Vec::new();
        let mut section_buf = [0u8; 4096];
        let start = Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);
        let mut seen_sections = std::collections::HashSet::new();
        let mut _read_count = 0u32;

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

            _read_count += 1;

            if n < 18 {
                continue;
            }

            // Filter for EIT present/following (table_id 0x4E) in userspace
            let table_id = section_buf[0];
            if table_id != EIT_PRESENT_FOLLOWING_ACTUAL {
                continue;
            }

            // Track sections to avoid duplicates
            let section_number = section_buf[6];
            let service_id = u16::from_be_bytes([section_buf[3], section_buf[4]]);
            let key = (service_id, section_number);

            if seen_sections.contains(&key) {
                continue;
            }
            seen_sections.insert(key);

            match parse_eit_section(&section_buf[..n]) {
                Ok((_sid, events)) => {
                    all_events.extend(events);
                }
                Err(_) => {}
            }
        }

        Ok(all_events)
    }
}
