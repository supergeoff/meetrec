#![allow(deprecated)] // cpal::DeviceTrait::name -> description(): refactor later.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait};

/// Sentinel name shown as the first entry in the device dropdown.
pub const SYSTEM_DEFAULT: &str = "System default";

/// List input device names. `"System default"` is the first item.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut out = vec![SYSTEM_DEFAULT.to_string()];
    if let Ok(devices) = host.input_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                if name != SYSTEM_DEFAULT {
                    out.push(name);
                }
            }
        }
    }
    out
}

/// Resolve a device name (or `"System default"`) to a `cpal::Device`.
pub fn resolve_device(name: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if name == SYSTEM_DEFAULT {
        return host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device"));
    }
    for d in host.input_devices()? {
        if let Ok(n) = d.name() {
            if n == name {
                return Ok(d);
            }
        }
    }
    // fall back to default if the previously-saved device was unplugged
    host.default_input_device()
        .ok_or_else(|| anyhow!("device '{}' not found and no default available", name))
}
