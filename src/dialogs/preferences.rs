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

use adw::prelude::*;
use gtk4::{self as gtk};

use crate::config;
use crate::volume::VolumeDisplay;

pub fn show(parent: Option<&impl IsA<gtk::Widget>>) {
    let dialog = adw::PreferencesDialog::new();
    let page = adw::PreferencesPage::new();

    let general = adw::PreferencesGroup::builder().title("General").build();

    let model = gtk::StringList::new(&["Decibel (dB)", "Percentage (%)"]);
    let selected = match VolumeDisplay::load() {
        VolumeDisplay::Decibel => 0,
        VolumeDisplay::Percentage => 1,
    };

    let vol_row = adw::ComboRow::builder()
        .title("Volume Display")
        .model(&model)
        .selected(selected)
        .build();

    vol_row.connect_selected_notify(|row| {
        let mode = match row.selected() {
            1 => VolumeDisplay::Percentage,
            _ => VolumeDisplay::Decibel,
        };
        mode.store();
    });

    general.add(&vol_row);

    let follow_row = adw::SwitchRow::builder()
        .title("System Default Follows Main")
        .subtitle("When Main is your default output, automatically change the system default to follow Direct and Virtual Surround states")
        .active(config::default_follows_main())
        .build();

    follow_row.connect_active_notify(|row| {
        config::set_default_follows_main(row.is_active());
    });

    general.add(&follow_row);
    page.add(&general);

    let pipewire = adw::PreferencesGroup::builder()
        .title("PipeWire Configuration")
        .build();

    let remove_row = adw::ActionRow::builder()
        .title("Remove Configuration")
        .subtitle("Virtual audio devices are removed after your next login")
        .build();

    let remove_btn = gtk::Button::builder()
        .label("Remove")
        .valign(gtk::Align::Center)
        .action_name("app.remove-config")
        .build();
    remove_btn.add_css_class("destructive-action");

    remove_row.add_suffix(&remove_btn);
    remove_row.set_activatable_widget(Some(&remove_btn));

    pipewire.add(&remove_row);
    page.add(&pipewire);

    dialog.add(&page);

    dialog.present(parent);
}
