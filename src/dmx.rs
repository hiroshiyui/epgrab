use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;

pub const DMX_FILTER_SIZE: usize = 16;
pub const DMX_IMMEDIATE_START: u32 = 4;

#[repr(C)]
pub struct DmxFilter {
    pub filter: [u8; DMX_FILTER_SIZE],
    pub mask: [u8; DMX_FILTER_SIZE],
    pub mode: [u8; DMX_FILTER_SIZE],
}

#[repr(C)]
pub struct DmxSctFilterParams {
    pub pid: u16,
    pub filter: DmxFilter,
    pub timeout: u32,
    pub flags: u32,
}

nix::ioctl_write_ptr!(dmx_set_filter, b'o', 43, DmxSctFilterParams);

/// Open the demux device and set a section filter for the given PID.
pub fn open_demux_with_filter(adapter: u32, pid: u16) -> Result<std::fs::File, String> {
    let path = format!("/dev/dvb/adapter{adapter}/demux0");
    let demux_file = OpenOptions::new()
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

    Ok(demux_file)
}
