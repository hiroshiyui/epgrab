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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn test_parse_channels_conf_valid() {
        let content = "公視:557000000:INVERSION_AUTO:BANDWIDTH_6_MHZ:FEC_AUTO:FEC_AUTO:QAM_64:TRANSMISSION_MODE_8K:GUARD_INTERVAL_1_8:HIERARCHY_NONE:4097:4098:1";
        let f = write_temp_file(content);
        let channels = parse_channels_conf(f.path()).unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "公視");
        assert_eq!(channels[0].frequency, 557000000);
        assert_eq!(channels[0].inversion, "INVERSION_AUTO");
        assert_eq!(channels[0].bandwidth, "BANDWIDTH_6_MHZ");
        assert_eq!(channels[0].fec_hp, "FEC_AUTO");
        assert_eq!(channels[0].fec_lp, "FEC_AUTO");
        assert_eq!(channels[0].modulation, "QAM_64");
        assert_eq!(channels[0].transmission_mode, "TRANSMISSION_MODE_8K");
        assert_eq!(channels[0].guard_interval, "GUARD_INTERVAL_1_8");
        assert_eq!(channels[0].hierarchy, "HIERARCHY_NONE");
        assert_eq!(channels[0].video_pid, 4097);
        assert_eq!(channels[0].audio_pid, 4098);
        assert_eq!(channels[0].service_id, 1);
    }

    #[test]
    fn test_parse_channels_conf_multiple() {
        let content = "\
CH1:557000000:INVERSION_AUTO:BANDWIDTH_6_MHZ:FEC_AUTO:FEC_AUTO:QAM_64:TRANSMISSION_MODE_8K:GUARD_INTERVAL_1_8:HIERARCHY_NONE:100:101:1
CH2:563000000:INVERSION_AUTO:BANDWIDTH_6_MHZ:FEC_AUTO:FEC_AUTO:QAM_64:TRANSMISSION_MODE_8K:GUARD_INTERVAL_1_8:HIERARCHY_NONE:200:201:2";
        let f = write_temp_file(content);
        let channels = parse_channels_conf(f.path()).unwrap();
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "CH1");
        assert_eq!(channels[1].name, "CH2");
    }

    #[test]
    fn test_parse_channels_conf_skips_comments_and_blanks() {
        let content = "\
# This is a comment

CH1:557000000:INVERSION_AUTO:BANDWIDTH_6_MHZ:FEC_AUTO:FEC_AUTO:QAM_64:TRANSMISSION_MODE_8K:GUARD_INTERVAL_1_8:HIERARCHY_NONE:100:101:1

# Another comment";
        let f = write_temp_file(content);
        let channels = parse_channels_conf(f.path()).unwrap();
        assert_eq!(channels.len(), 1);
    }

    #[test]
    fn test_parse_channels_conf_empty() {
        let f = write_temp_file("");
        let channels = parse_channels_conf(f.path()).unwrap();
        assert!(channels.is_empty());
    }

    #[test]
    fn test_parse_channels_conf_wrong_field_count() {
        let content = "CH1:557000000:INVERSION_AUTO";
        let f = write_temp_file(content);
        assert!(parse_channels_conf(f.path()).is_err());
    }

    #[test]
    fn test_parse_channels_conf_invalid_frequency() {
        let content = "CH1:notanumber:INVERSION_AUTO:BANDWIDTH_6_MHZ:FEC_AUTO:FEC_AUTO:QAM_64:TRANSMISSION_MODE_8K:GUARD_INTERVAL_1_8:HIERARCHY_NONE:100:101:1";
        let f = write_temp_file(content);
        assert!(parse_channels_conf(f.path()).is_err());
    }

    #[test]
    fn test_parse_channels_conf_invalid_pid() {
        let content = "CH1:557000000:INVERSION_AUTO:BANDWIDTH_6_MHZ:FEC_AUTO:FEC_AUTO:QAM_64:TRANSMISSION_MODE_8K:GUARD_INTERVAL_1_8:HIERARCHY_NONE:bad:101:1";
        let f = write_temp_file(content);
        assert!(parse_channels_conf(f.path()).is_err());
    }

    #[test]
    fn test_parse_channels_conf_nonexistent_file() {
        assert!(parse_channels_conf(Path::new("/nonexistent/path")).is_err());
    }
}
