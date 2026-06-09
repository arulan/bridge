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

use adw::subclass::prelude::*;
use gtk4::{self as gtk, CompositeTemplate};
use glib::subclass::InitializingObject;

#[derive(CompositeTemplate, Default)]
#[template(file = "../data/ui/window.ui")]
pub struct DashboardWindowImp {}

#[glib::object_subclass]
impl ObjectSubclass for DashboardWindowImp {
    const NAME: &'static str = "DashboardWindow";
    type Type = DashboardWindow;
    type ParentType = adw::ApplicationWindow;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}


impl ObjectImpl for DashboardWindowImp {}
impl WidgetImpl for DashboardWindowImp {}
impl WindowImpl for DashboardWindowImp {}
impl ApplicationWindowImpl for DashboardWindowImp {}
impl AdwApplicationWindowImpl for DashboardWindowImp {}

glib::wrapper! {
    pub struct DashboardWindow(ObjectSubclass<DashboardWindowImp>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap,
                    gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
                    gtk::Native, gtk::Root, gtk::ShortcutManager;
}


impl DashboardWindow {
    pub fn new(app: &adw::Application) -> Self {
        glib::Object::builder()
            .property("application", app)
            .build()
    }
}
