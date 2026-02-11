use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

const USB_IDS_PATHS: &[&str] = &[
    "/usr/share/misc/usb.ids",
    "/usr/share/hwdata/usb.ids",
];

pub struct DvbDevice {
    pub adapter_name: String,
    pub vendor_id: String,
    pub device_id: String,
    pub vendor_name: Option<String>,
    pub product_name: Option<String>,
}

fn find_usb_parent(path: &Path) -> Option<(String, String)> {
    let mut current = path.to_path_buf();
    loop {
        let vendor_path = current.join("idVendor");
        let product_path = current.join("idProduct");
        if vendor_path.exists() && product_path.exists() {
            let vendor = fs::read_to_string(&vendor_path).ok()?.trim().to_string();
            let product = fs::read_to_string(&product_path).ok()?.trim().to_string();
            return Some((vendor, product));
        }
        if !current.pop() {
            return None;
        }
    }
}

fn lookup_usb_names(vendor_id: &str, product_id: &str) -> (Option<String>, Option<String>) {
    let file = USB_IDS_PATHS
        .iter()
        .find_map(|p| fs::File::open(p).ok());

    let file = match file {
        Some(f) => f,
        None => return (None, None),
    };

    let reader = BufReader::new(file);
    let mut vendor_name = None;
    let mut in_target_vendor = false;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        // Skip comments and empty lines
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        if !line.starts_with('\t') {
            // Vendor line: "VVVV  Vendor Name"
            if in_target_vendor {
                // We've left our vendor's section without finding the product
                break;
            }
            if line.len() >= 4 && &line[..4] == vendor_id {
                vendor_name = Some(line[4..].trim().to_string());
                in_target_vendor = true;
            }
        } else if in_target_vendor && line.starts_with('\t') && !line.starts_with("\t\t") {
            // Product line: "\tPPPP  Product Name"
            let trimmed = line.trim_start_matches('\t');
            if trimmed.len() >= 4 && &trimmed[..4] == product_id {
                let product_name = trimmed[4..].trim().to_string();
                return (vendor_name, Some(product_name));
            }
        }
    }

    (vendor_name, None)
}

pub fn detect_devices() -> Vec<DvbDevice> {
    let dvb_class = Path::new("/sys/class/dvb");
    if !dvb_class.exists() {
        return Vec::new();
    }

    let entries = match fs::read_dir(dvb_class) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        if !name_str.contains("frontend") {
            continue;
        }

        let real_path = match fs::canonicalize(entry.path()) {
            Ok(p) => p,
            Err(_) => continue,
        };

        if let Some((vendor_id, device_id)) = find_usb_parent(&real_path) {
            let (vendor_name, product_name) = lookup_usb_names(&vendor_id, &device_id);
            devices.push(DvbDevice {
                adapter_name: name_str,
                vendor_id,
                device_id,
                vendor_name,
                product_name,
            });
        }
    }

    devices
}
