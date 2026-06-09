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
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk4::{self as gtk};

use crate::audio::backend::{self, PipeWireBackend};
use crate::audio::pw_config;
use crate::config;
use crate::dialogs::setup::SetupDialog;
use crate::window::DashboardWindow;

pub const APP_ID: &str = "io.github.arulan.Dashboard";

#[derive(Default)]
pub struct DashboardApplicationImp {
    window:  RefCell<Option<DashboardWindow>>,
    backend: RefCell<Option<Rc<RefCell<PipeWireBackend>>>>,
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

        let be = PipeWireBackend::new();
        backend::start(&be);
        *self.backend.borrow_mut() = Some(be);

        let window = DashboardWindow::new(app.upcast_ref::<adw::Application>());
        window.present();
        *self.window.borrow_mut() = Some(window);
    }

    fn shutdown(&self) {
        if let Some(be) = self.backend.borrow().as_ref() {
            backend::stop(be);
        }
        self.parent_shutdown();
    }
}

impl GtkApplicationImpl for DashboardApplicationImp {}
impl AdwApplicationImpl for DashboardApplicationImp {}

impl DashboardApplicationImp {
    fn show_setup_dialog(&self) {
        let Some(be) = self.backend.borrow().as_ref().map(Rc::clone) else { return };

        let win = self.window.borrow().clone();
        let hw_sinks = be.borrow().hw_sinks();
        let dialog = SetupDialog::new(hw_sinks, None, None, win.as_ref());

        dialog.connect_closure("approved", false, glib::closure_local!(
            move |d: SetupDialog| {
                let cfg = d.sink_config();
                config::store(&cfg);
                pw_config::write_config(&cfg);
            }
        ));
        
        dialog.present();
    }

    fn show_about_dialog(&self) {
        let about = adw::AboutDialog::builder()
            .application_name("Dashboard")
            .version(env!("CARGO_PKG_VERSION"))
            .developer_name("arulan")
            .developers(["arulan"])
            .copyright("© 2026 arulan")
            .license_type(gtk::License::Gpl30)
            .website("https://github.com/arulan/dashboard")
            .issue_url("https://github.com/arulan/dashboard/issues")
            .build();
        let parent = self.window.borrow().clone();
        about.present(parent.as_ref().map(|w| w.upcast_ref::<gtk::Widget>()));
    }
}

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

pub fn register_actions(app: &DashboardApplication) {
    let setup = gio::SimpleAction::new("setup", None);
    let app_c = app.clone();
    setup.connect_activate(move |_, _| app_c.imp().show_setup_dialog());
    app.add_action(&setup);

    let about = gio::SimpleAction::new("about", None);
    let app_c = app.clone();
    about.connect_activate(move |_, _| app_c.imp().show_about_dialog());
    app.add_action(&about);
}
