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

// Quick Switch configuration dialog

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk4::{self as gtk};

use crate::audio::hw_sink::HwSink;
use crate::config::Preset;
use crate::util::{hw_sink_factory, hw_sink_model, selected_hw_sink};

fn side_dropdown(sinks: &[HwSink], selected_hw: &str) -> gtk::DropDown {
    let model = hw_sink_model(sinks);
    let leave = HwSink {
        node_id: 0,
        name: String::new(),
        display_name: "Leave unchanged".to_owned(),
        device_api: String::new(),
        device_bus: String::new(),
        profile_name: String::new(),
        channels: 0,
        position: String::new(),
    };
    model.insert(0, &glib::BoxedAnyObject::new(leave));

    // Leave unchanged is a wildcard placeholder, the real hw sinks follow
    let idx = if selected_hw.is_empty() {
        0
    } else {
        sinks
            .iter()
            .position(|s| s.name == selected_hw)
            .map(|i| i as u32 + 1)
            .unwrap_or(0)
    };

    let dropdown = gtk::DropDown::builder()
        .model(&model)
        .selected(idx)
        .hexpand(true)
        .build();
    dropdown.set_factory(Some(&hw_sink_factory()));
    dropdown
}

fn sink_row(title: &str, dropdown: &gtk::DropDown) -> gtk::ListBoxRow {
    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();

    let label = gtk::Label::builder().label(title).xalign(0.0).build();
    label.add_css_class("caption");

    row_box.append(&label);
    row_box.append(dropdown);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&row_box));
    row.set_activatable(false);
    row
}

pub fn show(
    transient_for: Option<&impl IsA<gtk::Window>>,
    hw_sinks: Vec<HwSink>,
    presets: Vec<Preset>,
    on_saved: impl Fn(Vec<Preset>) + 'static,
) -> adw::Window {
    let mut edit = presets;
    let rest = if edit.len() > 2 {
        edit.split_off(2)
    } else {
        Vec::new()
    };
    while edit.len() < 2 {
        edit.push(Preset::new());
    }
    let both_valid = edit[0].is_valid() && edit[1].is_valid();
    let draft = Rc::new(RefCell::new(edit));

    let win = adw::Window::builder()
        .title("Quick Switch")
        .default_width(680)
        .modal(true)
        .resizable(false)
        .build();
    if let Some(parent) = transient_for {
        win.set_transient_for(Some(parent));
    }

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(false);
    toolbar.add_top_bar(&header);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| win.close()
    ));
    header.pack_start(&cancel);

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    save.set_sensitive(both_valid);
    header.pack_end(&save);
    win.set_default_widget(Some(&save));
    let save = Rc::new(save);

    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    let presets_header = adw::PreferencesGroup::new();
    presets_header.set_title("Presets");
    presets_header.set_description(Some(
        "Define two output configurations. A global shortcut swaps between them \
         when the current outputs match one of the presets.",
    ));
    outer.append(&presets_header);

    let cards = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .homogeneous(true)
        .spacing(12)
        .build();
    outer.append(&cards);

    let recompute = {
        let draft = Rc::clone(&draft);
        let save = Rc::clone(&save);
        Rc::new(move || {
            let d = draft.borrow();
            save.set_sensitive(d[0].is_valid() && d[1].is_valid());
        })
    };

    for i in 0..2 {
        let letter = if i == 0 { "A" } else { "B" };
        let default_name = if i == 0 { "Preset A" } else { "Preset B" };
        let preset = draft.borrow()[i].clone();

        let badge = gtk::Label::builder()
            .label(letter)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        badge.add_css_class("qs-badge");
        badge.add_css_class(if i == 0 { "qs-badge-a" } else { "qs-badge-b" });

        let name_row = adw::EntryRow::new();
        name_row.set_title("Preset name");
        name_row.set_max_length(10);
        name_row.set_text(if preset.name.is_empty() {
            default_name
        } else {
            &preset.name
        });
        name_row.add_prefix(&badge);

        let aux_dd = side_dropdown(&hw_sinks, &preset.aux_hw);
        let main_dd = side_dropdown(&hw_sinks, &preset.main_hw);

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .build();
        list.add_css_class("boxed-list");
        list.append(&name_row);
        list.append(&sink_row("Aux output", &aux_dd));
        list.append(&sink_row("Main output", &main_dd));

        let update = {
            let draft = Rc::clone(&draft);
            let recompute = Rc::clone(&recompute);
            let aux_dd = aux_dd.clone();
            let main_dd = main_dd.clone();
            let name_row = name_row.clone();
            move || {
                {
                    let mut d = draft.borrow_mut();
                    d[i].aux_hw = selected_hw_sink(&aux_dd)
                        .map(|s| s.name)
                        .unwrap_or_default();
                    d[i].main_hw = selected_hw_sink(&main_dd)
                        .map(|s| s.name)
                        .unwrap_or_default();
                    d[i].name = name_row.text().to_string();
                }
                recompute();
            }
        };

        let u = update.clone();
        aux_dd.connect_selected_notify(move |_| u());
        let u = update.clone();
        main_dd.connect_selected_notify(move |_| u());
        name_row.connect_changed(move |_| update());

        cards.append(&list);
    }

    toolbar.set_content(Some(&outer));
    win.set_content(Some(&toolbar));

    let on_saved = Rc::new(on_saved);
    let rest = Rc::new(rest);
    save.connect_clicked(glib::clone!(
        #[weak]
        win,
        #[strong]
        draft,
        #[strong]
        on_saved,
        #[strong]
        rest,
        move |_| {
            let mut out = draft.borrow().clone();
            out.extend(rest.iter().cloned());
            on_saved(out);
            win.close();
        }
    ));

    win.present();
    win
}
