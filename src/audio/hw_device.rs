// Copyright (C) 2026 arulan
//
// This file is part of Bridge.
//
// Bridge is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bridge is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Bridge. If not, see <https://www.gnu.org/licenses/>.

use pipewire::spa::utils::dict::DictRef;

#[derive(Clone, Debug)]
pub struct HwDevice {
    pub node_id: u32,
    pub name: String,
    pub display_name: String,
    pub device_api: String,
    pub device_bus: String,
    pub profile_name: String,
    pub channels: u32,
    pub position: String,
}

/// Builds HwDevice from a node's info props; None for non-sinks or our virtual
/// sinks. The info dict is the full property set
pub fn sink_from_props(node_id: u32, props: &DictRef) -> Option<HwDevice> {
    device_from_props(node_id, props, "Audio/Sink")
}

/// The mic side; None for anything that isn't a hardware source
pub fn source_from_props(node_id: u32, props: &DictRef) -> Option<HwDevice> {
    device_from_props(node_id, props, "Audio/Source")
}

fn device_from_props(node_id: u32, props: &DictRef, class: &str) -> Option<HwDevice> {
    if props.get("media.class") != Some(class) {
        return None;
    }

    let node_name = props.get("node.name").unwrap_or_default();
    if node_name.starts_with("bridge_") {
        return None;
    }

    let display_name = props
        .get("node.description")
        .or_else(|| props.get("device.name"))
        .unwrap_or(node_name)
        .to_owned();
    let device_api = props.get("device.api").unwrap_or_default().to_owned();
    let device_bus = props.get("device.bus").unwrap_or_default().to_owned();
    let profile_name = props
        .get("device.profile.name")
        .unwrap_or_default()
        .to_owned();
    let reported = props
        .get("audio.position")
        .map(normalize_position)
        .filter(|p| !p.is_empty());
    let channels = props
        .get("audio.channels")
        .and_then(|s| s.parse().ok())
        .or_else(|| reported.as_deref().map(count_positions))
        .unwrap_or(2);
    let position = reported.unwrap_or_else(|| default_position(channels).to_owned());

    Some(HwDevice {
        node_id,
        name: node_name.to_owned(),
        display_name,
        device_api,
        device_bus,
        profile_name,
        channels,
        position,
    })
}

impl HwDevice {
    // Label for connection hardware/transport type
    pub fn connection_label(&self) -> Option<&'static str> {
        let profile = self.profile_name.to_ascii_lowercase();
        let name = self.name.to_ascii_lowercase();

        let is_hdmi = profile.starts_with("hdmi") || name.contains(".hdmi-");
        let is_spdif = profile.contains("spdif")
            || profile.contains("iec958")
            || name.contains("spdif")
            || name.contains("iec958");

        if self.device_api == "bluez5" || self.device_bus == "bluetooth" {
            Some("Bluetooth")
        } else if is_hdmi {
            Some("HDMI / DP")
        } else if is_spdif {
            Some("S/PDIF")
        } else if self.device_bus == "usb" {
            Some("USB")
        } else if self.device_bus == "firewire" {
            Some("FireWire")
        } else if self.device_bus == "pci" || self.device_bus == "isa" {
            Some("Built-in")
        } else {
            None
        }
    }
}

/// Label for channel layout: "Mono", "Stereo", or the surround
/// "{full}.{lfe}" form (e.g. 5.1, 7.1, 2.1, and 4.0).
pub fn channel_layout_label(channels: u32, position: &str) -> String {
    match channels {
        0 => String::new(),
        1 => "Mono".into(),
        2 => "Stereo".into(),
        n => {
            let lfe = position.split(',').filter(|c| c.starts_with("LFE")).count() as u32;
            format!("{}.{} ch", n.saturating_sub(lfe), lfe)
        }
    }
}
pub fn strip_device_serial(description: &str) -> String {
    let Some(open) = description.find('(') else {
        return description.to_owned();
    };
    let Some(len) = description[open + 1..].find(')') else {
        return description.to_owned();
    };
    let inner = &description[open + 1..open + 1 + len];

    let serial = inner.len() >= 6 && inner.chars().all(|c| c.is_ascii_hexdigit());
    if !serial {
        return description.to_owned();
    }

    let mut out = String::with_capacity(description.len());
    out.push_str(description[..open].trim_end());
    let tail = description[open + len + 2..].trim_start();
    if !out.is_empty() && !tail.is_empty() {
        out.push(' ');
    }
    out.push_str(tail);
    out
}

// Fallback if audio.position isn't reported
fn default_position(channels: u32) -> &'static str {
    match channels {
        1 => "MONO",
        _ => "FL,FR",
    }
}

/// Devices in the order of the dropdowns
pub fn sorted_for_display(mut devices: Vec<HwDevice>) -> Vec<HwDevice> {
    let stripped: Vec<String> = devices
        .iter()
        .map(|d| strip_device_serial(&d.display_name))
        .collect();
    for (device, short) in devices.iter_mut().zip(&stripped) {
        if stripped.iter().filter(|other| *other == short).count() == 1 {
            device.display_name = short.clone();
        }
    }

    devices.sort_by_key(|d| d.display_name.to_lowercase());
    devices
}

// SPA uses space separated channels, such as "[ FL FR ]"; our is comma separated
fn normalize_position(raw: &str) -> String {
    raw.split(|c: char| c == ',' || c == '[' || c == ']' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

// position is already normalized
fn count_positions(position: &str) -> u32 {
    position.split(',').count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use pipewire::properties::properties;

    // sinks and sources come from the same registry
    // parser ignores the other class
    #[test]
    fn parsers_only_take_their_own_class() {
        let mic = properties! {
            "media.class"    => "Audio/Source",
            "node.name"      => "alsa_input.usb-headset",
            "audio.channels" => "1",
        };
        let speakers = properties! {
            "media.class"    => "Audio/Sink",
            "node.name"      => "alsa_output.pci-0000_00",
        };

        assert!(source_from_props(1, mic.dict()).is_some());
        assert!(sink_from_props(1, mic.dict()).is_none());
        assert!(sink_from_props(2, speakers.dict()).is_some());
        assert!(source_from_props(2, speakers.dict()).is_none());

        // a mono mic with no layout
        assert_eq!(source_from_props(1, mic.dict()).unwrap().position, "MONO");
    }

    #[test]
    fn skips_our_own_virtual_devices() {
        let ours = properties! {
            "media.class" => "Audio/Source",
            "node.name"   => "bridge_mic",
        };
        assert!(source_from_props(3, ours.dict()).is_none());
    }

    #[test]
    fn layout_and_count_agree() {
        let no_count = properties! {
            "media.class"    => "Audio/Source",
            "node.name"      => "alsa_input.usb-mic",
            "audio.position" => "[ MONO ]",
        };
        let mic = source_from_props(1, no_count.dict()).unwrap();
        assert_eq!((mic.channels, mic.position.as_str()), (1, "MONO"));

        let four_channel = properties! {
            "media.class"    => "Audio/Source",
            "node.name"      => "alsa_input.usb-scarlett",
            "audio.position" => "[ AUX0 AUX1 AUX2 AUX3 ]",
        };
        let interface = source_from_props(2, four_channel.dict()).unwrap();
        assert_eq!(interface.channels, 4);
    }

    #[test]
    fn strips_only_serials() {
        assert_eq!(
            strip_device_serial("ADI-2 DAC (12345678) Analog Stereo"),
            "ADI-2 DAC Analog Stereo"
        );
        assert_eq!(
            strip_device_serial("Scarlett 2i2 (A1B2C3D4)"),
            "Scarlett 2i2"
        );
        assert_eq!(
            strip_device_serial("Built-in Audio (HDMI 2)"),
            "Built-in Audio (HDMI 2)"
        );
        assert_eq!(
            strip_device_serial("GB202 High Definition"),
            "GB202 High Definition"
        );
        assert_eq!(strip_device_serial(""), "");
    }

    #[test]
    fn position_formats() {
        assert_eq!(normalize_position("[ FL FR ]"), "FL,FR");
        assert_eq!(normalize_position("FL,FR"), "FL,FR");
        assert_eq!(normalize_position("[ FL, FR, LFE ]"), "FL,FR,LFE");
    }

    fn sink(api: &str, bus: &str, profile: &str, name: &str) -> HwDevice {
        HwDevice {
            node_id: 0,
            name: name.to_owned(),
            display_name: String::new(),
            device_api: api.to_owned(),
            device_bus: bus.to_owned(),
            profile_name: profile.to_owned(),
            channels: 2,
            position: "FL,FR".to_owned(),
        }
    }

    #[test]
    fn connection_labels() {
        let bt = sink("bluez5", "", "a2dp-sink", "bluez_output.1");
        assert_eq!(bt.connection_label(), Some("Bluetooth"));

        let hdmi = sink(
            "alsa",
            "pci",
            "hdmi-stereo",
            "alsa_output.pci-0.1.hdmi-stereo",
        );
        assert_eq!(hdmi.connection_label(), Some("HDMI / DP"));

        let usb = sink(
            "alsa",
            "usb",
            "analog-stereo",
            "alsa_output.usb-RME.analog-stereo",
        );
        assert_eq!(usb.connection_label(), Some("USB"));

        let spdif_ucm = sink(
            "alsa",
            "usb",
            "HiFi: SPDIF: sink",
            "alsa_output.usb-Generic.HiFi__SPDIF__sink",
        );
        assert_eq!(spdif_ucm.connection_label(), Some("S/PDIF"));

        let spdif_acp = sink(
            "alsa",
            "pci",
            "iec958-stereo",
            "alsa_output.pci-0000_00_1f.3.iec958-stereo",
        );
        assert_eq!(spdif_acp.connection_label(), Some("S/PDIF"));

        let onboard = sink(
            "alsa",
            "pci",
            "analog-stereo",
            "alsa_output.pci-0000_00_1f.3.analog-stereo",
        );
        assert_eq!(onboard.connection_label(), Some("Built-in"));

        // People still using FireWire?
        let fw = sink(
            "alsa",
            "firewire",
            "analog-stereo",
            "alsa_output.firewire-Focusrite.analog-stereo",
        );
        assert_eq!(fw.connection_label(), Some("FireWire"));

        let unknown = sink("alsa", "", "", "alsa_output.platform-something");
        assert_eq!(unknown.connection_label(), None);
    }

    #[test]
    fn channel_labels() {
        assert_eq!(channel_layout_label(1, "MONO"), "Mono");
        assert_eq!(channel_layout_label(2, "FL,FR"), "Stereo");
        assert_eq!(channel_layout_label(3, "FL,FR,LFE"), "2.1 ch");
        assert_eq!(channel_layout_label(6, "FL,FR,FC,LFE,RL,RR"), "5.1 ch");
        assert_eq!(
            channel_layout_label(8, "FL,FR,FC,LFE,RL,RR,SL,SR"),
            "7.1 ch"
        );
        assert_eq!(channel_layout_label(4, "FL,FR,RL,RR"), "4.0 ch");
    }
}
