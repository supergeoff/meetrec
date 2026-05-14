use std::str::FromStr;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::DeviceId;

/// Sentinel id for the "System default" dropdown entry. Must not collide
/// with any real `DeviceId`'s `Display` format (cpal uses
/// "wasapi:…", "alsa:…", "coreaudio:…").
pub const SYSTEM_DEFAULT_ID: &str = "__system_default__";
pub const SYSTEM_DEFAULT_LABEL: &str = "System default";

#[derive(Clone, Debug)]
pub struct DeviceEntry {
    /// Either `SYSTEM_DEFAULT_ID` or the `Display` form of a `cpal::DeviceId`.
    pub id: String,
    /// Human-readable label for the dropdown.
    pub label: String,
}

impl DeviceEntry {
    pub fn system_default() -> Self {
        Self {
            id: SYSTEM_DEFAULT_ID.to_string(),
            label: SYSTEM_DEFAULT_LABEL.to_string(),
        }
    }
}

/// Enumerate input devices. The first entry is always "System default".
pub fn list_input_devices() -> Vec<DeviceEntry> {
    let mut out = vec![DeviceEntry::system_default()];
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else {
        return out;
    };
    for d in devices {
        let Ok(id) = d.id() else { continue };
        let id_str = id.to_string();
        let label = d
            .description()
            .ok()
            .map(|desc| desc.name().to_string())
            .unwrap_or_else(|| id_str.clone());
        out.push(DeviceEntry { id: id_str, label });
    }
    out
}

/// Resolve a stored device id (or the `SYSTEM_DEFAULT_ID` sentinel) to a
/// `cpal::Device`. Falls back to the host's default input device when the
/// saved id is missing, malformed (e.g. a leftover from a pre-`DeviceId`
/// config file), or refers to a device that's no longer plugged in.
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
