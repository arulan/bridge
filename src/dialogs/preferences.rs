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

    let step_adj = gtk::Adjustment::new(
        config::crossfade_step() as f64,
        config::CROSSFADE_STEP_MIN as f64,
        config::CROSSFADE_STEP_MAX as f64,
        1.0,
        1.0,
        0.0,
    );
    let step_row = adw::SpinRow::builder()
        .title("Crossfader Step")
        .subtitle("Percent of travel per key press")
        .adjustment(&step_adj)
        .build();

    step_row.connect_value_notify(|row| config::set_crossfade_step(row.value() as i32));

    general.add(&step_row);

    let follow_row = adw::SwitchRow::builder()
        .title("System Default Follows Main")
        .subtitle("When Main is your default output, automatically change the system default to follow Direct and Virtual Surround states")
        .active(config::default_follows_main())
        .build();

    follow_row.connect_active_notify(|row| {
        config::set_default_follows_main(row.is_active());
    });

    general.add(&follow_row);

    let routing_row = adw::SwitchRow::builder()
        .title("Open Routing on Startup")
        .subtitle("Expand the Routing tile when Bridge starts")
        .active(config::keep_routing_open())
        .build();

    routing_row.connect_active_notify(|row| {
        config::set_keep_routing_open(row.is_active());
    });

    general.add(&routing_row);
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

    pipewire.add(&remove_row);
    page.add(&pipewire);

    dialog.add(&page);

    dialog.present(parent);
}
