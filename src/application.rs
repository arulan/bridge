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

use crate::audio::backend::PipeWireBackend;
use crate::audio::pw_config;
use crate::config;
use crate::dialogs::preferences;
use crate::dialogs::setup::SetupDialog;
use crate::window::DashboardWindow;

pub const APP_ID: &str = "io.github.arulan.Dashboard";

// The GSettings SCHEMA_ID == APP_ID
pub fn settings() -> gio::Settings {
    gio::Settings::new(APP_ID)
}

#[derive(Default)]
pub struct DashboardApplicationImp {
    window:  RefCell<Option<DashboardWindow>>,
    backend: RefCell<Option<PipeWireBackend>>,
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
        be.start();
        *self.backend.borrow_mut() = Some(be.clone());

        let window = DashboardWindow::new(app.upcast_ref::<adw::Application>());
        window.setup(&be);
        *self.window.borrow_mut() = Some(window.clone());

        if config::is_configured() {
            window.present();
        } else {
            // Setup on first-run/!is_configured state; Do not show main window until after setup
            let app_c = app.clone();
            be.connect_sinks_ready(move |_| {
                app_c.imp().show_setup_dialog(true);
            });
        }
    }

    fn shutdown(&self) {
        if let Some(be) = self.backend.borrow().as_ref() {
            be.stop();
        }
        self.parent_shutdown();
    }
}

impl GtkApplicationImpl for DashboardApplicationImp {}
impl AdwApplicationImpl for DashboardApplicationImp {}

impl DashboardApplicationImp {
    fn show_setup_dialog(&self, first_run: bool) {
        let Some(be) = self.backend.borrow().clone() else { return };

        let win = self.window.borrow().clone();
        let hw_sinks = be.hw_sinks();
        let cfg = config::load();
        let (aux_id, main_id) = {
            let find_id = |name: &str| hw_sinks.iter().find(|s| s.name == name).map(|s| s.node_id);
            (find_id(&cfg.aux.hw_name), find_id(&cfg.main.hw_name))
        };
        let dialog = SetupDialog::new(hw_sinks, aux_id, main_id, win.as_ref());

        let win_c = win.clone();
        dialog.connect_closure("approved", false, glib::closure_local!(
            move |d: SetupDialog| {
                let cfg = d.sink_config();
                config::store(&cfg);
                pw_config::write_config(&cfg);
                if let Some(w) = &win_c {
                    w.populate_dropdowns();
                    w.present();
                }
            }
        ));

        // Quit app if first-run setup is cancelled
        if first_run {
            let app = self.obj().clone();
            dialog.connect_closure("declined", false, glib::closure_local!(
                move |_d: SetupDialog| {
                    app.quit();
                }
            ));
        }

        dialog.present();
    }

    fn show_preferences_dialog(&self) {
        let parent = self.window.borrow().clone();
        preferences::show(parent.as_ref());
    }

    fn show_shortcuts_dialog(&self) {
        let dialog = adw::ShortcutsDialog::new();

        let builder = gtk::Builder::from_string(include_str!("../data/ui/shortcuts.ui"));
        for id in ["section_crossfader", "section_application"] {
            if let Some(section) = builder.object::<adw::ShortcutsSection>(id) {
                dialog.add(section);
            }
        }

        let parent = self.window.borrow().clone();
        dialog.present(parent.as_ref().map(|w| w.upcast_ref::<gtk::Widget>()));
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
    setup.connect_activate(move |_, _| app_c.imp().show_setup_dialog(false));
    app.add_action(&setup);

    let preferences = gio::SimpleAction::new("preferences", None);
    let app_c = app.clone();
    preferences.connect_activate(move |_, _| app_c.imp().show_preferences_dialog());
    app.add_action(&preferences);
    app.set_accels_for_action("app.preferences", &["<Ctrl>comma"]);

    let shortcuts = gio::SimpleAction::new("show-help-overlay", None);
    let app_c = app.clone();
    shortcuts.connect_activate(move |_, _| app_c.imp().show_shortcuts_dialog());
    app.add_action(&shortcuts);
    app.set_accels_for_action("app.show-help-overlay", &["<Primary>question"]);

    let about = gio::SimpleAction::new("about", None);
    let app_c = app.clone();
    about.connect_activate(move |_, _| app_c.imp().show_about_dialog());
    app.add_action(&about);
}
