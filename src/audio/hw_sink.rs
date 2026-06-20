// Copyright (C) 2026 arulan
//
// This file is part of Dashboard.
//
// Dashboard is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Dashboard is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Dashboard. If not, see <https://www.gnu.org/licenses/>.

use pipewire::spa::utils::dict::DictRef;

#[derive(Clone, Debug)]
pub struct HwSink {
    pub node_id:      u32,
    pub name:         String,
    pub display_name: String,
    pub device_api:   String,
    pub device_bus:   String,
    pub profile_name: String,
    pub channels:     u32,
    pub position:     String,
}

/// Builds HwSink from a node's info props; None for non-sinks or our virtual
/// sinks. The info dict is the full property set
pub fn hw_sink_from_props(node_id: u32, props: &DictRef) -> Option<HwSink> {
    if props.get("media.class") != Some("Audio/Sink") {
        return None;
    }

    let node_name = props.get("node.name").unwrap_or_default();
    if node_name.starts_with("dashboard_") {
        return None;
    }

    let display_name = props.get("node.description")
        .or_else(|| props.get("device.name"))
        .unwrap_or(node_name)
        .to_owned();
    let device_api = props.get("device.api").unwrap_or_default().to_owned();
    let device_bus = props.get("device.bus").unwrap_or_default().to_owned();
    let profile_name = props.get("device.profile.name").unwrap_or_default().to_owned();
    let channels = props.get("audio.channels")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let position = props.get("audio.position")
        .map(normalize_position)
        .unwrap_or_else(|| "FL,FR".to_owned());

    Some(HwSink {
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

impl HwSink {

    // Label for connection hardware/transport type
    pub fn connection_label(&self) -> Option<&'static str> {
        let profile = self.profile_name.to_ascii_lowercase();
        let name = self.name.to_ascii_lowercase();

        let is_hdmi = profile.starts_with("hdmi") || name.contains(".hdmi-");
        let is_spdif = profile.contains("spdif") || profile.contains("iec958")
            || name.contains("spdif") || name.contains("iec958");

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

// SPA uses space separated channels, such as "[ FL FR ]"; our is comma separated
fn normalize_position(raw: &str) -> String {
    raw.split(|c: char| c == ',' || c == '[' || c == ']' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_formats() {
        assert_eq!(normalize_position("[ FL FR ]"), "FL,FR");
        assert_eq!(normalize_position("FL,FR"), "FL,FR");
        assert_eq!(normalize_position("[ FL, FR, LFE ]"), "FL,FR,LFE");
    }

    fn sink(api: &str, bus: &str, profile: &str, name: &str) -> HwSink {
        HwSink {
            node_id:      0,
            name:         name.to_owned(),
            display_name: String::new(),
            device_api:   api.to_owned(),
            device_bus:   bus.to_owned(),
            profile_name: profile.to_owned(),
            channels:     2,
            position:     "FL,FR".to_owned(),
        }
    }

    #[test]
    fn connection_labels() {
        let bt = sink("bluez5", "", "a2dp-sink", "bluez_output.1");
        assert_eq!(bt.connection_label(), Some("Bluetooth"));

        let hdmi = sink("alsa", "pci", "hdmi-stereo", "alsa_output.pci-0.1.hdmi-stereo");
        assert_eq!(hdmi.connection_label(), Some("HDMI / DP"));

        let usb = sink("alsa", "usb", "analog-stereo", "alsa_output.usb-RME.analog-stereo");
        assert_eq!(usb.connection_label(), Some("USB"));

        let spdif_ucm = sink("alsa", "usb", "HiFi: SPDIF: sink", "alsa_output.usb-Generic.HiFi__SPDIF__sink");
        assert_eq!(spdif_ucm.connection_label(), Some("S/PDIF"));

        let spdif_acp = sink("alsa", "pci", "iec958-stereo", "alsa_output.pci-0000_00_1f.3.iec958-stereo");
        assert_eq!(spdif_acp.connection_label(), Some("S/PDIF"));

        let onboard = sink("alsa", "pci", "analog-stereo", "alsa_output.pci-0000_00_1f.3.analog-stereo");
        assert_eq!(onboard.connection_label(), Some("Built-in"));

        // People still using FireWire?
        let fw = sink("alsa", "firewire", "analog-stereo", "alsa_output.firewire-Focusrite.analog-stereo");
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
        assert_eq!(channel_layout_label(8, "FL,FR,FC,LFE,RL,RR,SL,SR"), "7.1 ch");
        assert_eq!(channel_layout_label(4, "FL,FR,RL,RR"), "4.0 ch");
    }
}
