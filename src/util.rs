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

use gtk4::{self as gtk};
use gtk::prelude::*;

use crate::audio::hw_sink::HwSink;

/// ListStore of HwSinks
pub fn hw_sink_model(sinks: &[HwSink]) -> gio::ListStore {
    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    for sink in sinks {
        store.append(&glib::BoxedAnyObject::new(sink.clone()));
    }
    store
}

pub fn selected_hw_sink(dropdown: &gtk::DropDown) -> Option<HwSink> {
    dropdown
        .selected_item()
        .and_downcast::<glib::BoxedAnyObject>()
        .map(|boxed| boxed.borrow::<HwSink>().clone())
}

/// Dropdown entries show device display name;
/// ellipsized to avoid stretching the dropdown
pub fn hw_sink_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();

    factory.connect_setup(|_, obj| {
        let item = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .xalign(0.0)
            .build();
        item.set_child(Some(&label));
    });

    factory.connect_bind(|_, obj| {
        let item = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let label = item.child().unwrap().downcast::<gtk::Label>().unwrap();
        if let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() {
            label.set_label(&boxed.borrow::<HwSink>().display_name);
        }
    });
    factory
}

/// The default.audio.sink metadata value is SPA-JSON like { "name": "<node.name>" },
/// not a bare string, so pull the name out before comparing it to our sink names.
pub fn parse_default_name(value: &str) -> Option<String> {
    let after_key   = value.split_once("\"name\"")?.1;
    let after_colon = after_key.split_once(':')?.1;
    let open        = after_colon.find('"')? + 1;
    let rest        = &after_colon[open..];
    let close       = rest.find('"')?;
    Some(rest[..close].to_owned())
}

#[cfg(test)]
mod tests {
    use super::parse_default_name;

    #[test]
    fn pulls_name_from_json() {
        assert_eq!(parse_default_name(r#"{"name":"dashboard_main"}"#).as_deref(), Some("dashboard_main"));
        assert_eq!(parse_default_name(r#"{ "name": "alsa_output.pci-0000" }"#).as_deref(), Some("alsa_output.pci-0000"));
    }

    #[test]
    fn none_when_no_name() {
        assert_eq!(parse_default_name(r#"{"other":"x"}"#), None);
        assert_eq!(parse_default_name(""), None);
    }
}
