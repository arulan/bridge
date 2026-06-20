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
    page.add(&general);
    dialog.add(&page);

    dialog.present(parent);
}
