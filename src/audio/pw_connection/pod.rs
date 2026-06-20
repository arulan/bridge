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

// Builds the Props parameters we pass onto owned sinks for volume and mute control

use std::io::Cursor;

use pipewire as pw;
use pw::spa;
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Property, PropertyFlags, Value, ValueArray};

// For applying channelVolumes & mute. The volume carries its own channel count
// since the array width has to match the node
pub(super) fn set_node_props(node: &pw::node::Node, volume: Option<(f32, u32)>, mute: Option<bool>) {
    let bytes = props_pod(volume, mute);
    if let Some(pod) = Pod::from_bytes(&bytes) {
        node.set_param(spa::param::ParamType::Props, 0, pod);
    }
}

// Props pod for channelVolumes & mute
fn props_pod(volume: Option<(f32, u32)>, mute: Option<bool>) -> Vec<u8> {
    let mut props = Vec::new();

    if let Some((v, channels)) = volume {
        props.push(Property {
            key:   spa::sys::SPA_PROP_channelVolumes,
            flags: PropertyFlags::empty(),
            value: Value::ValueArray(ValueArray::Float(vec![v; channels.max(1) as usize])),
        });
    }

    if let Some(m) = mute {
        props.push(Property {
            key:   spa::sys::SPA_PROP_mute,
            flags: PropertyFlags::empty(),
            value: Value::Bool(m),
        });
    }

    let object = Value::Object(Object {
        type_:      spa::sys::SPA_TYPE_OBJECT_Props,
        id:         spa::sys::SPA_PARAM_Props,
        properties: props,
    });

    PodSerializer::serialize(Cursor::new(Vec::new()), &object)
        .expect("failed to serialize Props pod")
        .0
        .into_inner()
}
