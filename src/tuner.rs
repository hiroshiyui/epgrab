use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::thread;
use std::time::Duration;

use crate::channel::Channel;

// DVB v5 API property command IDs
const DTV_TUNE: u32 = 1;
const DTV_CLEAR: u32 = 2;
const DTV_FREQUENCY: u32 = 3;
const DTV_MODULATION: u32 = 4;
const DTV_BANDWIDTH_HZ: u32 = 5;
const DTV_INVERSION: u32 = 6;
const DTV_DELIVERY_SYSTEM: u32 = 17;
const DTV_CODE_RATE_HP: u32 = 36;
const DTV_CODE_RATE_LP: u32 = 37;
const DTV_GUARD_INTERVAL: u32 = 38;
const DTV_TRANSMISSION_MODE: u32 = 39;
const DTV_HIERARCHY: u32 = 40;

// Delivery system
const SYS_DVBT: u32 = 3;

// Frontend status flags
const FE_HAS_LOCK: u32 = 0x10;

// Kernel struct: dtv_property (76 bytes, packed)
//   cmd: u32, reserved: [u32; 3], u: union(56 bytes), result: i32
#[repr(C, packed)]
struct DtvProperty {
    cmd: u32,
    reserved: [u32; 3],
    data: u32,
    _padding: [u8; 52], // remaining union space (56 - 4)
    result: i32,
}

// Kernel struct: dtv_properties (16 bytes on 64-bit)
//   num: u32, (4 bytes padding), props: *mut DtvProperty
#[repr(C)]
struct DtvProperties {
    num: u32,
    props: *mut DtvProperty,
}

// ioctl declarations
nix::ioctl_write_ptr!(fe_set_property, b'o', 82, DtvProperties);
nix::ioctl_read!(fe_read_status, b'o', 69, u32);

impl DtvProperty {
    fn new(cmd: u32, data: u32) -> Self {
        DtvProperty {
            cmd,
            reserved: [0; 3],
            data,
            _padding: [0; 52],
            result: 0,
        }
    }
}

fn parse_bandwidth(s: &str) -> Result<u32, String> {
    match s {
        "BANDWIDTH_6_MHZ" => Ok(6_000_000),
        "BANDWIDTH_7_MHZ" => Ok(7_000_000),
        "BANDWIDTH_8_MHZ" => Ok(8_000_000),
        "BANDWIDTH_5_MHZ" => Ok(5_000_000),
        "BANDWIDTH_10_MHZ" => Ok(10_000_000),
        "BANDWIDTH_1_712_MHZ" => Ok(1_712_000),
        "BANDWIDTH_AUTO" => Ok(0),
        _ => Err(format!("Unknown bandwidth: {s}")),
    }
}

fn parse_modulation(s: &str) -> Result<u32, String> {
    match s {
        "QPSK" => Ok(0),
        "QAM_16" => Ok(1),
        "QAM_32" => Ok(2),
        "QAM_64" => Ok(3),
        "QAM_128" => Ok(4),
        "QAM_256" => Ok(5),
        "QAM_AUTO" => Ok(6),
        _ => Err(format!("Unknown modulation: {s}")),
    }
}

fn parse_fec(s: &str) -> Result<u32, String> {
    match s {
        "FEC_NONE" => Ok(0),
        "FEC_1_2" => Ok(1),
        "FEC_2_3" => Ok(2),
        "FEC_3_4" => Ok(3),
        "FEC_4_5" => Ok(4),
        "FEC_5_6" => Ok(5),
        "FEC_6_7" => Ok(6),
        "FEC_7_8" => Ok(7),
        "FEC_8_9" => Ok(8),
        "FEC_AUTO" => Ok(9),
        _ => Err(format!("Unknown FEC: {s}")),
    }
}

fn parse_inversion(s: &str) -> Result<u32, String> {
    match s {
        "INVERSION_OFF" => Ok(0),
        "INVERSION_ON" => Ok(1),
        "INVERSION_AUTO" => Ok(2),
        _ => Err(format!("Unknown inversion: {s}")),
    }
}

fn parse_transmission_mode(s: &str) -> Result<u32, String> {
    match s {
        "TRANSMISSION_MODE_2K" => Ok(0),
        "TRANSMISSION_MODE_8K" => Ok(1),
        "TRANSMISSION_MODE_AUTO" => Ok(2),
        "TRANSMISSION_MODE_4K" => Ok(3),
        "TRANSMISSION_MODE_1K" => Ok(4),
        "TRANSMISSION_MODE_16K" => Ok(5),
        "TRANSMISSION_MODE_32K" => Ok(6),
        _ => Err(format!("Unknown transmission mode: {s}")),
    }
}

fn parse_guard_interval(s: &str) -> Result<u32, String> {
    match s {
        "GUARD_INTERVAL_1_32" => Ok(0),
        "GUARD_INTERVAL_1_16" => Ok(1),
        "GUARD_INTERVAL_1_8" => Ok(2),
        "GUARD_INTERVAL_1_4" => Ok(3),
        "GUARD_INTERVAL_AUTO" => Ok(4),
        _ => Err(format!("Unknown guard interval: {s}")),
    }
}

fn parse_hierarchy(s: &str) -> Result<u32, String> {
    match s {
        "HIERARCHY_NONE" => Ok(0),
        "HIERARCHY_1" => Ok(1),
        "HIERARCHY_2" => Ok(2),
        "HIERARCHY_4" => Ok(3),
        "HIERARCHY_AUTO" => Ok(4),
        _ => Err(format!("Unknown hierarchy: {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_bandwidth ---

    #[test]
    fn test_parse_bandwidth_all_values() {
        assert_eq!(parse_bandwidth("BANDWIDTH_6_MHZ").unwrap(), 6_000_000);
        assert_eq!(parse_bandwidth("BANDWIDTH_7_MHZ").unwrap(), 7_000_000);
        assert_eq!(parse_bandwidth("BANDWIDTH_8_MHZ").unwrap(), 8_000_000);
        assert_eq!(parse_bandwidth("BANDWIDTH_5_MHZ").unwrap(), 5_000_000);
        assert_eq!(parse_bandwidth("BANDWIDTH_10_MHZ").unwrap(), 10_000_000);
        assert_eq!(parse_bandwidth("BANDWIDTH_1_712_MHZ").unwrap(), 1_712_000);
        assert_eq!(parse_bandwidth("BANDWIDTH_AUTO").unwrap(), 0);
    }

    #[test]
    fn test_parse_bandwidth_unknown() {
        assert!(parse_bandwidth("INVALID").is_err());
    }

    // --- parse_modulation ---

    #[test]
    fn test_parse_modulation_all_values() {
        assert_eq!(parse_modulation("QPSK").unwrap(), 0);
        assert_eq!(parse_modulation("QAM_16").unwrap(), 1);
        assert_eq!(parse_modulation("QAM_32").unwrap(), 2);
        assert_eq!(parse_modulation("QAM_64").unwrap(), 3);
        assert_eq!(parse_modulation("QAM_128").unwrap(), 4);
        assert_eq!(parse_modulation("QAM_256").unwrap(), 5);
        assert_eq!(parse_modulation("QAM_AUTO").unwrap(), 6);
    }

    #[test]
    fn test_parse_modulation_unknown() {
        assert!(parse_modulation("INVALID").is_err());
    }

    // --- parse_fec ---

    #[test]
    fn test_parse_fec_all_values() {
        assert_eq!(parse_fec("FEC_NONE").unwrap(), 0);
        assert_eq!(parse_fec("FEC_1_2").unwrap(), 1);
        assert_eq!(parse_fec("FEC_2_3").unwrap(), 2);
        assert_eq!(parse_fec("FEC_3_4").unwrap(), 3);
        assert_eq!(parse_fec("FEC_4_5").unwrap(), 4);
        assert_eq!(parse_fec("FEC_5_6").unwrap(), 5);
        assert_eq!(parse_fec("FEC_6_7").unwrap(), 6);
        assert_eq!(parse_fec("FEC_7_8").unwrap(), 7);
        assert_eq!(parse_fec("FEC_8_9").unwrap(), 8);
        assert_eq!(parse_fec("FEC_AUTO").unwrap(), 9);
    }

    #[test]
    fn test_parse_fec_unknown() {
        assert!(parse_fec("INVALID").is_err());
    }

    // --- parse_inversion ---

    #[test]
    fn test_parse_inversion_all_values() {
        assert_eq!(parse_inversion("INVERSION_OFF").unwrap(), 0);
        assert_eq!(parse_inversion("INVERSION_ON").unwrap(), 1);
        assert_eq!(parse_inversion("INVERSION_AUTO").unwrap(), 2);
    }

    #[test]
    fn test_parse_inversion_unknown() {
        assert!(parse_inversion("INVALID").is_err());
    }

    // --- parse_transmission_mode ---

    #[test]
    fn test_parse_transmission_mode_all_values() {
        assert_eq!(parse_transmission_mode("TRANSMISSION_MODE_2K").unwrap(), 0);
        assert_eq!(parse_transmission_mode("TRANSMISSION_MODE_8K").unwrap(), 1);
        assert_eq!(parse_transmission_mode("TRANSMISSION_MODE_AUTO").unwrap(), 2);
        assert_eq!(parse_transmission_mode("TRANSMISSION_MODE_4K").unwrap(), 3);
        assert_eq!(parse_transmission_mode("TRANSMISSION_MODE_1K").unwrap(), 4);
        assert_eq!(parse_transmission_mode("TRANSMISSION_MODE_16K").unwrap(), 5);
        assert_eq!(parse_transmission_mode("TRANSMISSION_MODE_32K").unwrap(), 6);
    }

    #[test]
    fn test_parse_transmission_mode_unknown() {
        assert!(parse_transmission_mode("INVALID").is_err());
    }

    // --- parse_guard_interval ---

    #[test]
    fn test_parse_guard_interval_all_values() {
        assert_eq!(parse_guard_interval("GUARD_INTERVAL_1_32").unwrap(), 0);
        assert_eq!(parse_guard_interval("GUARD_INTERVAL_1_16").unwrap(), 1);
        assert_eq!(parse_guard_interval("GUARD_INTERVAL_1_8").unwrap(), 2);
        assert_eq!(parse_guard_interval("GUARD_INTERVAL_1_4").unwrap(), 3);
        assert_eq!(parse_guard_interval("GUARD_INTERVAL_AUTO").unwrap(), 4);
    }

    #[test]
    fn test_parse_guard_interval_unknown() {
        assert!(parse_guard_interval("INVALID").is_err());
    }

    // --- parse_hierarchy ---

    #[test]
    fn test_parse_hierarchy_all_values() {
        assert_eq!(parse_hierarchy("HIERARCHY_NONE").unwrap(), 0);
        assert_eq!(parse_hierarchy("HIERARCHY_1").unwrap(), 1);
        assert_eq!(parse_hierarchy("HIERARCHY_2").unwrap(), 2);
        assert_eq!(parse_hierarchy("HIERARCHY_4").unwrap(), 3);
        assert_eq!(parse_hierarchy("HIERARCHY_AUTO").unwrap(), 4);
    }

    #[test]
    fn test_parse_hierarchy_unknown() {
        assert!(parse_hierarchy("INVALID").is_err());
    }
}

pub struct Tuner {
    fe_file: std::fs::File,
}

impl Tuner {
    pub fn open(adapter: u32) -> Result<Self, String> {
        let path = format!("/dev/dvb/adapter{adapter}/frontend0");
        let fe_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("Failed to open {path}: {e}"))?;
        Ok(Tuner { fe_file })
    }

    pub fn has_lock(&self) -> bool {
        let fd = self.fe_file.as_raw_fd();
        let mut status: u32 = 0;
        unsafe {
            if fe_read_status(fd, &mut status).is_err() {
                return false;
            }
        }
        status & FE_HAS_LOCK != 0
    }

    pub fn tune(&self, channel: &Channel) -> Result<(), String> {
        let fd = self.fe_file.as_raw_fd();

        // Clear previous tuning
        let mut clear_prop = DtvProperty::new(DTV_CLEAR, 0);
        let mut clear_props = DtvProperties {
            num: 1,
            props: &mut clear_prop,
        };
        unsafe {
            fe_set_property(fd, &mut clear_props)
                .map_err(|e| format!("DTV_CLEAR failed: {e}"))?;
        }

        // Build tuning properties
        let mut props = [
            DtvProperty::new(DTV_DELIVERY_SYSTEM, SYS_DVBT),
            DtvProperty::new(DTV_FREQUENCY, channel.frequency as u32),
            DtvProperty::new(DTV_BANDWIDTH_HZ, parse_bandwidth(&channel.bandwidth)?),
            DtvProperty::new(DTV_MODULATION, parse_modulation(&channel.modulation)?),
            DtvProperty::new(DTV_CODE_RATE_HP, parse_fec(&channel.fec_hp)?),
            DtvProperty::new(DTV_CODE_RATE_LP, parse_fec(&channel.fec_lp)?),
            DtvProperty::new(DTV_INVERSION, parse_inversion(&channel.inversion)?),
            DtvProperty::new(DTV_TRANSMISSION_MODE, parse_transmission_mode(&channel.transmission_mode)?),
            DtvProperty::new(DTV_GUARD_INTERVAL, parse_guard_interval(&channel.guard_interval)?),
            DtvProperty::new(DTV_HIERARCHY, parse_hierarchy(&channel.hierarchy)?),
            DtvProperty::new(DTV_TUNE, 0),
        ];

        let mut dtv_props = DtvProperties {
            num: props.len() as u32,
            props: props.as_mut_ptr(),
        };

        unsafe {
            fe_set_property(fd, &mut dtv_props)
                .map_err(|e| format!("FE_SET_PROPERTY failed: {e}"))?;
        }

        // Poll for lock (up to 10 seconds)
        for i in 0..100 {
            let mut status: u32 = 0;
            unsafe {
                fe_read_status(fd, &mut status)
                    .map_err(|e| format!("FE_READ_STATUS failed: {e}"))?;
            }
            if status & FE_HAS_LOCK != 0 {
                println!("Frontend locked after {}ms", (i + 1) * 100);
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }

        Err("Tuning timed out: no lock after 10 seconds".to_string())
    }
}
