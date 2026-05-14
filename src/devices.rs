use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{DeviceId, InterfaceType};
use directories::ProjectDirs;

pub const SYSTEM_DEFAULT_ID: &str = "__system_default__";
pub const SYSTEM_DEFAULT_LABEL: &str = "System default";

#[derive(Clone, Debug)]
pub struct DeviceEntry {
    pub id: String,
    pub label: String,
}

impl DeviceEntry {
    pub fn system_default(suffix: Option<&str>) -> Self {
        let label = match suffix {
            Some(s) if !s.is_empty() => format!("{SYSTEM_DEFAULT_LABEL} — {s}"),
            _ => SYSTEM_DEFAULT_LABEL.to_string(),
        };
        Self {
            id: SYSTEM_DEFAULT_ID.to_string(),
            label,
        }
    }
}

/// Enumerate input devices. The first entry is always "System default",
/// suffixed with the current default's name so the user can tell what
/// it points at.
///
/// A diagnostic dump of everything cpal returned (id, name, manufacturer,
/// device_type, interface_type, direction) is written to
/// `<config_dir>/devices.txt` on each call — useful when an expected
/// physical device is missing from the dropdown.
pub fn list_input_devices() -> Vec<DeviceEntry> {
    let host = cpal::default_host();

    let mut diag = String::new();
    let _ = writeln!(diag, "cpal host: {:?}", host.id());

    let default = host.default_input_device();
    let default_name = default
        .as_ref()
        .and_then(|d| d.description().ok().map(|desc| desc.name().to_string()));
    let default_id = default.as_ref().and_then(|d| d.id().ok()).map(|id| id.to_string());
    let _ = writeln!(
        diag,
        "default input: id={:?} name={:?}",
        default_id, default_name
    );

    let mut out = vec![DeviceEntry::system_default(default_name.as_deref())];

    let devices = match host.input_devices() {
        Ok(it) => it,
        Err(e) => {
            let _ = writeln!(diag, "ERROR host.input_devices() failed: {e:?}");
            write_diag(&diag);
            return out;
        }
    };

    let mut count = 0usize;
    for d in devices {
        count += 1;
        let id_str = d
            .id()
            .ok()
            .map(|id| id.to_string())
            .unwrap_or_else(|| format!("<no-id-#{count}>"));
        match d.description() {
            Ok(desc) => {
                let _ = writeln!(
                    diag,
                    "[{count}] id={} name={:?} manufacturer={:?} device_type={:?} interface_type={:?} direction={:?}",
                    id_str,
                    desc.name(),
                    desc.manufacturer(),
                    desc.device_type(),
                    desc.interface_type(),
                    desc.direction()
                );
                out.push(DeviceEntry {
                    id: id_str,
                    label: build_label(&desc),
                });
            }
            Err(e) => {
                let _ = writeln!(diag, "[{count}] id={} description() error: {e:?}", id_str);
                out.push(DeviceEntry {
                    id: id_str.clone(),
                    label: id_str,
                });
            }
        }
    }
    let _ = writeln!(diag, "total: {count} input device(s) enumerated");

    deduplicate_labels(&mut out);

    if let Some(path) = diag_path() {
        let _ = writeln!(diag, "(diagnostic written to {})", path.display());
    }
    write_diag(&diag);

    out
}

/// Resolve a stored device id (or the `SYSTEM_DEFAULT_ID` sentinel) to a
/// `cpal::Device`. Falls back to the host's default input device when the
/// saved id is missing, malformed, or refers to a device that's no longer
/// plugged in.
pub fn resolve_device(id: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if id == SYSTEM_DEFAULT_ID {
        return host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device available"));
    }
    if let Ok(target) = DeviceId::from_str(id) {
        for d in host.input_devices()? {
            if d.id().ok().as_ref() == Some(&target) {
                return Ok(d);
            }
        }
        log::warn!("saved device id '{id}' not found among current inputs — using default");
    } else {
        log::warn!("saved device id '{id}' is not a valid cpal::DeviceId — using default");
    }
    host.default_input_device()
        .ok_or_else(|| anyhow!("device '{id}' not found and no default available"))
}

// ── label building ──────────────────────────────────────────────────────

fn build_label(desc: &cpal::DeviceDescription) -> String {
    let name = desc.name();
    let iface = humanize_interface(desc.interface_type());
    let manuf = desc.manufacturer();

    let manuf_useful = manuf.filter(|m| !name.to_lowercase().contains(&m.to_lowercase()));

    match (iface, manuf_useful) {
        (Some(i), Some(m)) => format!("{} ({} · {})", name, i, m),
        (Some(i), None) => format!("{} ({})", name, i),
        (None, Some(m)) => format!("{} ({})", name, m),
        (None, None) => name.to_string(),
    }
}

fn humanize_interface(it: InterfaceType) -> Option<&'static str> {
    Some(match it {
        InterfaceType::BuiltIn => "built-in",
        InterfaceType::Usb => "USB",
        InterfaceType::Bluetooth => "Bluetooth",
        InterfaceType::Hdmi => "HDMI",
        InterfaceType::DisplayPort => "DisplayPort",
        InterfaceType::Thunderbolt => "Thunderbolt",
        InterfaceType::FireWire => "FireWire",
        InterfaceType::Pci => "PCI",
        InterfaceType::Spdif => "S/PDIF",
        InterfaceType::Line => "line",
        InterfaceType::Network => "network",
        InterfaceType::Virtual => "virtual",
        InterfaceType::Aggregate => "aggregate",
        _ => return None,
    })
}

/// When two real-device entries share the same label after enrichment,
/// suffix each colliding entry with a short tail of its DeviceId so the
/// user can pick one. The leading "System default" entry is left alone.
fn deduplicate_labels(entries: &mut [DeviceEntry]) {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for e in entries.iter().skip(1) {
        *counts.entry(e.label.as_str()).or_insert(0) += 1;
    }
    let collisions: Vec<String> = counts
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(k, _)| k.to_string())
        .collect();
    for e in entries.iter_mut().skip(1) {
        if collisions.iter().any(|c| c == &e.label) {
            let tail: String = e.id.chars().rev().take(8).collect();
            let tail: String = tail.chars().rev().collect();
            e.label = format!("{} [{}]", e.label, tail);
        }
    }
}

// ── diagnostic dump ─────────────────────────────────────────────────────

fn diag_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "geoffroy", "meetrec")
        .map(|d| d.config_dir().join("devices.txt"))
}

fn write_diag(content: &str) {
    if let Some(path) = diag_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, content) {
            log::warn!("could not write {}: {e:#}", path.display());
        }
    }
}
