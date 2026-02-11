use std::fs;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::io::AsRawFd;

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

use crate::channel::Channel;
use crate::eit::decode_dvb_text;

// Demux constants
const DMX_FILTER_SIZE: usize = 16;
const DMX_IMMEDIATE_START: u32 = 4;

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

// --- Generic section reader ---

/// Read all sections for a given PID/table_id, collecting until we have
/// section_number 0 through last_section_number. Returns all raw section buffers.
fn read_all_sections(adapter: u32, pid: u16, table_id: u8, timeout_secs: u64) -> Result<Vec<Vec<u8>>, String> {
    let path = format!("/dev/dvb/adapter{adapter}/demux0");
    let mut demux_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("Failed to open {path}: {e}"))?;

    let fd = demux_file.as_raw_fd();

    let params = DmxSctFilterParams {
        pid,
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
