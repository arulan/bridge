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

use adw::prelude::*;
use gtk4::{self as gtk};

use crate::audio::routing::StreamInfo;

pub(super) fn streams_popover(title: &str) -> (gtk::Popover, gtk::ListBox) {
    let header = gtk::Label::builder().label(title).xalign(0.0).build();
    header.add_css_class("heading");

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");

    // wrap the card in a box to avoid clipping the shadows
    let list_wrap = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(3)
        .margin_bottom(3)
        .margin_start(3)
        .margin_end(3)
        .build();
    list_wrap.append(&list);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(420)
        .child(&list_wrap)
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .width_request(300)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    content.append(&header);
    content.append(&scrolled);

    let popover = gtk::Popover::builder().child(&content).build();
    (popover, list)
}

pub(super) fn fill_streams(list: &gtk::ListBox, streams: &[StreamInfo]) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    if streams.is_empty() {
        let row = adw::ActionRow::builder().title("No streams").build();
        row.set_sensitive(false);
        list.append(&row);
        return;
    }

    let mut sorted: Vec<&StreamInfo> = streams.iter().collect();
    sorted.sort_by_key(|s| sort_key(s));

    for info in sorted {
        list.append(&stream_row(info));
    }
}

// Streams sort by app name
fn sort_key(info: &StreamInfo) -> String {
    info.app_name
        .clone()
        .or_else(|| info.binary.clone())
        .unwrap_or_default()
        .to_lowercase()
}

// One row; app name over binary + media.name
pub(super) fn stream_row(info: &StreamInfo) -> adw::ActionRow {
    let title = info
        .app_name
        .clone()
        .or_else(|| info.binary.clone())
        .unwrap_or_else(|| "Unknown application".to_owned());

    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&title).as_str())
        .build();
    row.set_title_lines(1);

    let mut parts: Vec<&str> = Vec::new();
    if info.app_name.is_some()
        && let Some(bin) = &info.binary
    {
        parts.push(bin);
    }
    if let Some(media) = &info.media_name {
        parts.push(media);
    }
    if !parts.is_empty() {
        let subtitle = parts.join(" · ");
        row.set_subtitle(&glib::markup_escape_text(&subtitle));
        row.set_subtitle_lines(1);
        row.set_tooltip_text(Some(&subtitle));
    }

    row
}
