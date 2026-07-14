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

use gtk::prelude::*;
use gtk4::{self as gtk};

use crate::audio::backend::PipeWireBackend;
use crate::audio::hw_sink::HwSink;
use crate::audio::routing::RuleTarget;

pub struct RouteTarget {
    pub label: String,
    pub target: RuleTarget,
}

pub fn route_targets(hw_sinks: &[HwSink]) -> Vec<RouteTarget> {
    let mut out = vec![
        RouteTarget {
            label: "Aux".to_owned(),
            target: RuleTarget::Aux,
        },
        RouteTarget {
            label: "Main".to_owned(),
            target: RuleTarget::Main,
        },
    ];
    for sink in hw_sinks {
        out.push(RouteTarget {
            label: sink.display_name.clone(),
            target: RuleTarget::DirectHw(sink.name.clone()),
        });
    }
    out
}

pub fn row_level_meter() -> gtk::LevelBar {
    let bar = gtk::LevelBar::builder()
        .min_value(0.0)
        .max_value(1.0)
        .width_request(60)
        .valign(gtk::Align::Center)
        .build();
    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_LOW));
    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_HIGH));
    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_FULL));
    bar.add_css_class("level-meter");
    bar
}

pub fn drive_stream_meters(backend: &PipeWireBackend, meters: &[(gtk::LevelBar, Vec<u32>)]) {
    const SMOOTHING: f64 = 0.3;
    for (bar, ids) in meters {
        let peak = ids
            .iter()
            .map(|&id| backend.stream_peak(id) as f64)
            .fold(0.0, f64::max);
        let val = (peak * SMOOTHING + bar.value() * (1.0 - SMOOTHING)).clamp(0.0, 1.0);
        bar.set_value(val);
    }
}

pub fn stream_count(n: usize) -> String {
    match n {
        0 => "No streams".to_owned(),
        1 => "1 stream".to_owned(),
        _ => format!("{n} streams"),
    }
}

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
    let after_key = value.split_once("\"name\"")?.1;
    let after_colon = after_key.split_once(':')?.1;
    let open = after_colon.find('"')? + 1;
    let rest = &after_colon[open..];
    let close = rest.find('"')?;
    Some(rest[..close].to_owned())
}

/// GlobalShorcuts on GNOME replies with trigger_description
/// e.g. "Press <Shift><Control>Left"
/// Drop the leading verb
/// TODO: Check if this holds true across other localizations
pub fn accelerator_from_trigger_description(desc: &str) -> String {
    let desc = desc.trim();
    if desc.is_empty() {
        return String::new();
    }
    if let Some(i) = desc.find('<') {
        desc[i..].to_owned()
    } else {
        desc.rsplit(char::is_whitespace)
            .next()
            .unwrap_or(desc)
            .to_owned()
    }
}

// used by the Setup and Virtual Surround dialogs
pub fn make_device_row(label_text: &str, dropdown: &gtk::DropDown) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .valign(gtk::Align::Center)
        .build();
    let lbl = gtk::Label::builder()
        .label(label_text)
        .xalign(0.0)
        .hexpand(true)
        .build();
    row.append(&lbl);
    row.append(dropdown);
    row
}

// an expandable preview of the created conf file
pub fn make_file_row(path: &str, content: &str) -> gtk::Box {
    let home = glib::home_dir().to_string_lossy().into_owned();
    let display_path = path.replacen(&home, "~", 1);

    // TODO: Check with GNOME HIG on EllipsizeMode recommendation
    let lbl = gtk::Label::builder()
        .label(&display_path)
        .xalign(0.0)
        .hexpand(true)
        .max_width_chars(1)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .tooltip_text(&display_path)
        .build();
    lbl.add_css_class("monospace");
    lbl.add_css_class("caption");

    let expander = gtk::Expander::new(None);
    expander.set_label_widget(Some(&lbl));

    let tv = gtk::TextView::builder()
        .editable(false)
        .monospace(true)
        .cursor_visible(false)
        .top_margin(10)
        .bottom_margin(10)
        .left_margin(12)
        .right_margin(12)
        .build();
    tv.buffer().set_text(content.trim());

    let sw = gtk::ScrolledWindow::builder()
        .propagate_natural_height(true)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&tv)
        .build();

    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&sw));
    frame.set_margin_top(6);
    expander.set_child(Some(&frame));

    let boxw = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    boxw.append(&expander);
    boxw
}

#[cfg(test)]
mod tests {
    use super::parse_default_name;

    #[test]
    fn pulls_name_from_json() {
        assert_eq!(
            parse_default_name(r#"{"name":"bridge_main"}"#).as_deref(),
            Some("bridge_main")
        );
        assert_eq!(
            parse_default_name(r#"{ "name": "alsa_output.pci-0000" }"#).as_deref(),
            Some("alsa_output.pci-0000")
        );
    }

    #[test]
    fn none_when_no_name() {
        assert_eq!(parse_default_name(r#"{"other":"x"}"#), None);
        assert_eq!(parse_default_name(""), None);
    }
}
