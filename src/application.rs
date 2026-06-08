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

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk4::{self as gtk};

use crate::window::DashboardWindow;

pub const APP_ID: &str = "io.github.arulan.Dashboard";

#[derive(Default)]
pub struct DashboardApplicationImp {
    window: RefCell<Option<DashboardWindow>>,
}

#[glib::object_subclass]
impl ObjectSubclass for DashboardApplicationImp {
    const NAME: &'static str = "DashboardApplication";
    type Type = DashboardApplication;
    type ParentType = adw::Application;
}

impl ObjectImpl for DashboardApplicationImp {}

impl ApplicationImpl for DashboardApplicationImp {
    fn activate(&self) {
        self.parent_activate();

        if let Some(window) = self.window.borrow().as_ref() {
            window.present();
            return;
        }

        let app = self.obj();
        let window = DashboardWindow::new(app.upcast_ref::<adw::Application>());
        window.present();
        *self.window.borrow_mut() = Some(window);
    }
}

impl GtkApplicationImpl for DashboardApplicationImp {}
impl AdwApplicationImpl for DashboardApplicationImp {}

// cannot be dereferenced?
glib::wrapper! {
    pub struct DashboardApplication(ObjectSubclass<DashboardApplicationImp>)
        @extends adw::Application, gtk::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl DashboardApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", APP_ID)
            .property("flags", gio::ApplicationFlags::empty())
            .build()
    }
}
