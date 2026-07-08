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
use crate::dialogs::surround::SurroundDialog;
use crate::shortcuts::{self, ShortcutsPortal};
use crate::util;
use crate::window::DashboardWindow;

// fallback for cargo; comes from meson at build time now
pub const APP_ID: &str = match option_env!("APP_ID") {
    Some(id) => id,
    None => "io.github.arulan.Dashboard",
};

pub const RESOURCES_FILE: Option<&str> = option_env!("RESOURCES_FILE");

// The GSettings SCHEMA_ID == APP_ID
pub fn settings() -> gio::Settings {
    gio::Settings::new(APP_ID)
}

#[derive(Default)]
pub struct DashboardApplicationImp {
    window: RefCell<Option<DashboardWindow>>,
    backend: RefCell<Option<PipeWireBackend>>,
    shortcuts: RefCell<Option<ShortcutsPortal>>,
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

        // Global shortcuts portal
        let portal = ShortcutsPortal::new();
        if let Some(conn) = app.dbus_connection() {
            portal.start(conn);
        }
        window.bind_shortcuts(&portal);
        *self.shortcuts.borrow_mut() = Some(portal);

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
        if let Some(portal) = self.shortcuts.borrow().as_ref() {
            portal.stop();
        }
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
        let Some(be) = self.backend.borrow().clone() else {
            return;
        };

        let win = self.window.borrow().clone();
        let hw_sinks = be.hw_sinks();
        let cfg = config::load();
        let (aux_id, main_id) = {
            let find_id = |name: &str| hw_sinks.iter().find(|s| s.name == name).map(|s| s.node_id);
            (find_id(&cfg.aux.hw_name), find_id(&cfg.main.hw_name))
        };
        let dialog = SetupDialog::new(hw_sinks, aux_id, main_id, win.as_ref());

        let win_c = win.clone();
        let be_c = be.clone();
        dialog.connect_closure(
            "approved",
            false,
            glib::closure_local!(move |d: SetupDialog| {
                let cfg = d.sink_config();
                config::store(&cfg);
                pw_config::write_config(&cfg);

                be_c.recreate_temp_sinks();
                if let Some(w) = &win_c {
                    w.populate_dropdowns();
                    w.present();
                }
            }),
        );

        // Quit app if first-run setup is cancelled
        if first_run {
            let app = self.obj().clone();
            dialog.connect_closure(
                "declined",
                false,
                glib::closure_local!(move |_d: SetupDialog| {
                    app.quit();
                }),
            );
        }

        dialog.present();
    }

    fn show_surround_dialog(&self) {
        let Some(be) = self.backend.borrow().clone() else {
            return;
        };
        let win = self.window.borrow().clone();
        let current = config::load_surround();
        let dialog = SurroundDialog::new(be.hw_sinks(), &current, win.as_ref());

        let win_c = win.clone();
        dialog.connect_closure(
            "approved",
            false,
            glib::closure_local!(move |d: SurroundDialog| {
                let (Some(sink), Some(source)) = (d.selected_sink(), d.hrir_source()) else {
                    return;
                };
                let hrir_path = match pw_config::import_hrir(&source) {
                    Ok(dest) => dest.to_string_lossy().into_owned(),
                    Err(e) => {
                        eprintln!("surround: failed to import HRIR: {e}");
                        return;
                    }
                };
                let old = config::load_surround();
                config::store_surround(&config::SurroundConfig {
                    hrir_path: hrir_path.clone(),
                    hw_name: sink.name.clone(),
                    display_name: sink.display_name.clone(),
                });
                pw_config::write_surround_config(&hrir_path, &sink.name);
                if let Some(w) = &win_c {
                    if old.hw_name != sink.name || old.hrir_path != hrir_path {
                        w.note_surround_reconfig();
                    }
                    w.refresh_surround();
                }
            }),
        );

        let win_c = win.clone();
        dialog.connect_closure(
            "reset",
            false,
            glib::closure_local!(move |_d: SurroundDialog| {
                config::clear_surround();
                pw_config::remove_surround_config();
                if let Some(w) = &win_c {
                    w.refresh_surround();
                }
            }),
        );

        dialog.present();
    }

    fn show_preferences_dialog(&self) {
        let parent = self.window.borrow().clone();
        preferences::show(parent.as_ref());
    }

    fn show_shortcuts_dialog(&self) {
        let dialog = adw::ShortcutsDialog::new();

        let active = self
            .shortcuts
            .borrow()
            .as_ref()
            .is_some_and(|p| p.is_active());
        let global = adw::ShortcutsSection::new(Some("Global Shortcuts"));
        let items: Vec<adw::ShortcutsItem> = shortcuts::SHORTCUTS
            .iter()
            .map(|(_, desc, _)| {
                let item = adw::ShortcutsItem::new(desc, "");
                item.set_subtitle(if active {
                    "Set in your desktop's shortcut settings"
                } else {
                    "Unavailable"
                });
                global.add(item.clone());
                item
            })
            .collect();
        dialog.add(global);

        if let Some(portal) = self.shortcuts.borrow().clone() {
            portal.list_shortcuts(move |list| {
                // GNOME implementation sends trigger descriptions per shortcut
                // KDE unfortunately does not appear to do so
                let map: std::collections::HashMap<String, String> = list
                    .into_iter()
                    .map(|(id, _desc, trigger)| (id, trigger))
                    .collect();
                for (i, (id, _, _)) in shortcuts::SHORTCUTS.iter().enumerate() {
                    let Some(item) = items.get(i) else { continue };
                    let accel = map
                        .get(*id)
                        .map(|t| util::accelerator_from_trigger_description(t))
                        .unwrap_or_default();
                    if !accel.is_empty() {
                        item.set_accelerator(&accel);
                        item.set_subtitle("");
                    }
                }
            });
        }

        let builder = gtk::Builder::from_resource("/io/github/arulan/Dashboard/ui/shortcuts.ui");
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

    let surround = gio::SimpleAction::new("surround", None);
    let app_c = app.clone();
    surround.connect_activate(move |_, _| app_c.imp().show_surround_dialog());
    app.add_action(&surround);

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
