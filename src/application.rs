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

use std::cell::RefCell;
use std::process::Command;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk4::{self as gtk};

use crate::audio::backend::PipeWireBackend;
use crate::audio::pw_config;
use crate::config;
use crate::dialogs::preferences;
use crate::dialogs::quick_switch;
use crate::dialogs::setup::SetupDialog;
use crate::dialogs::surround::SurroundDialog;
use crate::shortcuts::{self, ShortcutsPortal};
use crate::util;
use crate::window::BridgeWindow;

// fallback for cargo; comes from meson at build time now
pub const APP_ID: &str = match option_env!("APP_ID") {
    Some(id) => id,
    None => "io.github.arulan.Bridge",
};

pub const RESOURCES_FILE: Option<&str> = option_env!("RESOURCES_FILE");

// The GSettings SCHEMA_ID == APP_ID
pub fn settings() -> gio::Settings {
    gio::Settings::new(APP_ID)
}

fn show_error_alert(parent: Option<&BridgeWindow>, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response("ok", "OK");
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    dialog.present(parent);
}

#[derive(Default)]
pub struct BridgeApplicationImp {
    window: RefCell<Option<BridgeWindow>>,
    backend: RefCell<Option<PipeWireBackend>>,
    shortcuts: RefCell<Option<ShortcutsPortal>>,
}

#[glib::object_subclass]
impl ObjectSubclass for BridgeApplicationImp {
    const NAME: &'static str = "BridgeApplication";
    type Type = BridgeApplication;
    type ParentType = adw::Application;
}

impl ObjectImpl for BridgeApplicationImp {}

impl ApplicationImpl for BridgeApplicationImp {
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

        let window = BridgeWindow::new(app.upcast_ref::<adw::Application>());
        window.setup(&be);
        *self.window.borrow_mut() = Some(window.clone());

        // Global shortcuts portal
        let portal = ShortcutsPortal::new();
        window.bind_shortcuts(&portal);
        *self.shortcuts.borrow_mut() = Some(portal);

        window.present();

        if config::is_configured() {
            self.start_shortcuts();
        } else {
            // Setup on first-run/!is_configured state
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

impl GtkApplicationImpl for BridgeApplicationImp {}
impl AdwApplicationImpl for BridgeApplicationImp {}

impl BridgeApplicationImp {
    fn start_shortcuts(&self) {
        let Some(conn) = self.obj().dbus_connection() else {
            return;
        };
        if let Some(portal) = self.shortcuts.borrow().as_ref() {
            portal.start(conn);
        }
    }

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
        let dialog = SetupDialog::new(hw_sinks, aux_id, main_id);

        let win_c = win.clone();
        let be_c = be.clone();
        let app_c = self.obj().clone();
        dialog.connect_closure(
            "approved",
            false,
            glib::closure_local!(move |d: SetupDialog| {
                let cfg = d.sink_config();
                config::store(&cfg);
                if let Err(e) = pw_config::write_config(&cfg) {
                    eprintln!("setup: failed to write config: {e}");
                    show_error_alert(
                        win_c.as_ref(),
                        "Error Writing Configuration",
                        &format!(
                            "The virtual outputs will work for this session, but Bridge couldn't write the \
                             PipeWire configuration, so they won't come back after you log out.\n\n{e}"
                        ),
                    );
                }

                be_c.recreate_temp_sinks();
                if let Some(w) = &win_c {
                    w.populate_dropdowns();
                    w.present();
                }
                if first_run {
                    app_c.imp().start_shortcuts();
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

        dialog.present(win.as_ref());
    }

    fn show_surround_dialog(&self) {
        let Some(be) = self.backend.borrow().clone() else {
            return;
        };
        let win = self.window.borrow().clone();
        let current = config::load_surround();
        let dialog = SurroundDialog::new(be.hw_sinks(), &current);

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
                        show_error_alert(
                            win_c.as_ref(),
                            "Error Importing HRIR File",
                            &format!(
                                "Bridge couldn't import the HRIR file into place, so Virtual \
                                 Surround was not enabled.\n\n{e}"
                            ),
                        );
                        return;
                    }
                };
                let old = config::load_surround();
                config::store_surround(&config::SurroundConfig {
                    hrir_path: hrir_path.clone(),
                    hw_name: sink.name.clone(),
                    display_name: sink.display_name.clone(),
                });
                if let Err(e) = pw_config::write_surround_config(&hrir_path, &sink.name) {
                    eprintln!("surround: failed to write config: {e}");
                    show_error_alert(
                        win_c.as_ref(),
                        "Error Writing Surround Configuration",
                        &format!(
                            "The HRIR file was imported, but Bridge couldn't write the PipeWire \
                             configuration, so Virtual Surround was not enabled.\n\n{e}"
                        ),
                    );
                    return;
                }
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

        dialog.present(win.as_ref());
    }

    fn show_quick_switch_dialog(&self) {
        let Some(be) = self.backend.borrow().clone() else {
            return;
        };
        let win = self.window.borrow().clone();
        let refresh_win = win.clone();
        quick_switch::show(
            win.as_ref(),
            be.hw_sinks(),
            config::load_presets(),
            move |presets| {
                config::store_presets(&presets);
                if let Some(w) = &refresh_win {
                    w.update_qs_toggle();
                }
            },
        );
    }

    fn show_preferences_dialog(&self) {
        let parent = self.window.borrow().clone();
        preferences::show(parent.as_ref());
    }

    fn show_remove_config_dialog(&self) {
        let win = self.window.borrow().clone();

        let dialog = adw::AlertDialog::new(
            Some("Remove PipeWire Configuration?"),
            Some(
                "This removes the configuration files Bridge created, including any Virtual \
                 Surround setup, and returns Bridge to first-run setup. Your imported HRIR files \
                 are left in place.\n\nThe changes take effect after your next login.",
            ),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("remove", "Remove");
        dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        dialog.connect_response(
            None,
            glib::clone!(
                #[strong]
                win,
                move |_: &adw::AlertDialog, response: &str| {
                    if response != "remove" {
                        return;
                    }
                    pw_config::remove_config();
                    config::clear_sinks();
                    config::clear_surround();
                    pw_config::remove_surround_config();
                    if let Some(w) = &win {
                        w.refresh_surround();
                    }
                }
            ),
        );

        dialog.present(win.as_ref());
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

        let builder = gtk::Builder::from_resource("/io/github/arulan/Bridge/ui/shortcuts.ui");
        for id in ["section_crossfader", "section_application"] {
            if let Some(section) = builder.object::<adw::ShortcutsSection>(id) {
                dialog.add(section);
            }
        }

        let parent = self.window.borrow().clone();
        dialog.present(parent.as_ref().map(|w| w.upcast_ref::<gtk::Widget>()));
    }

    fn show_about_dialog(&self) {
        let debug_info = collect_diagnostic_info();
        let about = adw::AboutDialog::builder()
            .application_name("Bridge")
            .application_icon(APP_ID)
            .version(env!("CARGO_PKG_VERSION"))
            .developer_name("arulan")
            .developers(["arulan"])
            .copyright("© 2026 arulan")
            .license_type(gtk::License::Gpl30)
            .website("https://github.com/arulan/bridge")
            .issue_url("https://github.com/arulan/bridge/issues")
            .debug_info(&debug_info)
            .debug_info_filename("bridge-diagnostic.txt")
            .build();
        let parent = self.window.borrow().clone();
        about.present(parent.as_ref().map(|w| w.upcast_ref::<gtk::Widget>()));
    }
}

// cannot be dereferenced?
glib::wrapper! {
    pub struct BridgeApplication(ObjectSubclass<BridgeApplicationImp>)
        @extends adw::Application, gtk::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl BridgeApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", APP_ID)
            .property("flags", gio::ApplicationFlags::empty())
            .build()
    }
}

pub fn register_actions(app: &BridgeApplication) {
    let setup = gio::SimpleAction::new("setup", None);
    let app_c = app.clone();
    setup.connect_activate(move |_, _| app_c.imp().show_setup_dialog(false));
    app.add_action(&setup);

    let surround = gio::SimpleAction::new("surround", None);
    let app_c = app.clone();
    surround.connect_activate(move |_, _| app_c.imp().show_surround_dialog());
    app.add_action(&surround);

    let quick_switch = gio::SimpleAction::new("quick-switch", None);
    let app_c = app.clone();
    quick_switch.connect_activate(move |_, _| app_c.imp().show_quick_switch_dialog());
    app.add_action(&quick_switch);

    let preferences = gio::SimpleAction::new("preferences", None);
    let app_c = app.clone();
    preferences.connect_activate(move |_, _| app_c.imp().show_preferences_dialog());
    app.add_action(&preferences);
    app.set_accels_for_action("app.preferences", &["<Ctrl>comma"]);

    let remove_config = gio::SimpleAction::new("remove-config", None);
    let app_c = app.clone();
    remove_config.connect_activate(move |_, _| app_c.imp().show_remove_config_dialog());
    app.add_action(&remove_config);

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

fn run_cmd(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .map(|o| {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            let err = String::from_utf8_lossy(&o.stderr);
            if !err.is_empty() {
                s.push_str(&err);
            }
            s.trim_end().to_owned()
        })
        .unwrap_or_else(|e| format!("(failed to run: {e})"))
}

fn read_file(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|_| "(not found)".to_owned())
}

fn collect_diagnostic_info() -> String {
    let kernel = run_cmd("uname", &["-r"]);
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| std::env::var("DESKTOP_SESSION"))
        .unwrap_or_else(|_| "(unknown)".to_owned());

    let pw_ver = run_cmd("pw-cli", &["--version"]);
    let defaults = run_cmd("pw-metadata", &["-n", "default"]);
    let links = run_cmd("pw-link", &["-l"]);
    let nodes = run_cmd("pw-cli", &["ls", "Node"]);

    let pw_conf = read_file(&pw_config::config_file());
    let surround_conf = {
        let p = pw_config::surround_config_file();
        if p.exists() {
            read_file(&p)
        } else {
            "(not present)".to_owned()
        }
    };

    let sinks = config::load();
    let surround = config::load_surround();

    let vol = crate::volume::VolumeDisplay::load();
    let s = settings();

    let sink_line = |def: &config::SinkDef| {
        format!(
            "\"{}\" [{}] {}ch {}",
            def.display_name, def.hw_name, def.channels, def.position
        )
    };

    format!(
        "=== Bridge Diagnostic Report ===\n\
         App version: {ver}\n\
         \n\
         --- System ---\n\
         Kernel:  {kernel}\n\
         Desktop: {desktop}\n\
         \n\
         --- PipeWire ---\n\
         {pw_ver}\n\
         \n\
         --- Default sink/source (pw-metadata) ---\n\
         {defaults}\n\
         \n\
         --- Links (pw-link -l) ---\n\
         {links}\n\
         \n\
         --- Nodes (pw-cli ls Node) ---\n\
         {nodes}\n\
         \n\
         --- Bridge Aux/Main config ({pw_path}) ---\n\
         {pw_conf}\n\
         \n\
         --- Bridge Surround config ---\n\
         {surround_conf}\n\
         \n\
         --- Config summary ---\n\
         Aux:  {aux}\n\
         Main: {main}\n\
         Surround: hrir=\"{hrir}\" hw=\"{s_hw}\" name=\"{s_name}\" active={s_active}\n\
         Prefs: default-follows-main={follows_main} keep-routing-open={open_routing} volume-display={vol}\n\
         Window: {w}x{h} maximized={max}",
        ver = env!("CARGO_PKG_VERSION"),
        pw_path = pw_config::config_file().display(),
        aux = sink_line(&sinks.aux),
        main = sink_line(&sinks.main),
        hrir = surround.hrir_path,
        s_hw = surround.hw_name,
        s_name = surround.display_name,
        s_active = config::surround_active(),
        follows_main = config::default_follows_main(),
        open_routing = config::keep_routing_open(),
        vol = vol.as_key(),
        w = s.int("window-width"),
        h = s.int("window-height"),
        max = s.boolean("is-maximized"),
    )
}
