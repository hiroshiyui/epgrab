use std::fs;
use std::path::Path;

#[allow(dead_code)]
pub struct Channel {
    pub name: String,
    pub frequency: u64,
    pub inversion: String,
    pub bandwidth: String,
    pub fec_hp: String,
    pub fec_lp: String,
    pub modulation: String,
    pub transmission_mode: String,
    pub guard_interval: String,
    pub hierarchy: String,
    pub video_pid: u16,
    pub audio_pid: u16,
    pub service_id: u16,
}

pub fn parse_channels_conf(path: &Path) -> Result<Vec<Channel>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    let mut channels = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() != 13 {
            return Err(format!(
                "Line {}: expected 13 fields, got {}",
                line_num + 1,
                fields.len()
            ));
        }

        let frequency = fields[1].parse::<u64>().map_err(|e| {
            format!("Line {}: invalid frequency '{}': {e}", line_num + 1, fields[1])
        })?;
        let video_pid = fields[10].parse::<u16>().map_err(|e| {
            format!("Line {}: invalid video PID '{}': {e}", line_num + 1, fields[10])
        })?;
        let audio_pid = fields[11].parse::<u16>().map_err(|e| {
            format!("Line {}: invalid audio PID '{}': {e}", line_num + 1, fields[11])
        })?;
        let service_id = fields[12].parse::<u16>().map_err(|e| {
            format!("Line {}: invalid service ID '{}': {e}", line_num + 1, fields[12])
        })?;

        channels.push(Channel {
            name: fields[0].to_string(),
            frequency,
            inversion: fields[2].to_string(),
            bandwidth: fields[3].to_string(),
            fec_hp: fields[4].to_string(),
            fec_lp: fields[5].to_string(),
            modulation: fields[6].to_string(),
            transmission_mode: fields[7].to_string(),
            guard_interval: fields[8].to_string(),
            hierarchy: fields[9].to_string(),
            video_pid,
            audio_pid,
            service_id,
        });
    }

    Ok(channels)
}
