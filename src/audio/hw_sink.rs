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

#[derive(Clone, Debug)]
pub struct HwSink {
    pub node_id:      u32,
    pub name:         String,
    pub display_name: String,
    pub device_api:   String,
    pub channels:     u32,
    pub position:     String,
}

/// Builds HwSink from a WP node; None for non-sinks or our virtual sinks
pub fn hw_sink_from_node(node: &glib::Object) -> Option<HwSink> {
    use crate::wp;

    let media_class = wp::node_prop(node, "media.class").unwrap_or_default();
    if media_class != "Audio/Sink" {
        return None;
    }

    // Filter out app loopbacks from hw list using role prop or name as fallback
    if wp::node_prop(node, "dashboard.role").is_some() {
        return None;
    }
    let node_name = wp::node_prop(node, "node.name").unwrap_or_default();
    if node_name.starts_with("dashboard_") {
        return None;
    }

    // Builds our HwSink
    let node_id = wp::bound_id(node);
    let display_name = wp::node_prop(node, "node.description")
        .or_else(|| wp::node_prop(node, "device.name"))
        .unwrap_or_else(|| node_name.clone());
    let device_api = wp::node_prop(node, "device.api").unwrap_or_default();
    let channels = wp::node_prop(node, "audio.channels")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let position = wp::node_prop(node, "audio.position").unwrap_or_else(|| "FL,FR".to_owned());

    Some(HwSink { node_id, name: node_name, display_name, device_api, channels, position })
}
