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

pub fn ellipsize_string_factory() -> gtk::SignalListItemFactory {
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
        if let Some(s) = item.item().and_downcast::<gtk::StringObject>() {
            label.set_label(&s.string());
        }
    });
    factory
}
