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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use gio::prelude::*;
use glib::subclass::Signal;
use glib::subclass::prelude::*;

// (id, description, preferred_trigger)
pub const SHORTCUTS: &[(&str, &str, &str)] = &[
    ("step-left", "Step Towards Aux", "CTRL+SHIFT+Left"),
    ("step-right", "Step Towards Main", "CTRL+SHIFT+Right"),
    ("reset", "Reset Balance", "CTRL+SHIFT+Down"),
    (
        "quick-switch-outputs",
        "Switch Output Preset",
        "CTRL+SHIFT+p",
    ),
];

const PORTAL_BUS: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS_IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST_IFACE: &str = "org.freedesktop.portal.Request";
const SESSION_IFACE: &str = "org.freedesktop.portal.Session";

const MAX_BIND_RETRIES: u32 = 3;
const BIND_RETRY_BACKOFF: Duration = Duration::from_secs(1);

const MAX_CREATE_RETRIES: u32 = 3;
const CREATE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct ShortcutsPortalImp {
    conn: RefCell<Option<gio::DBusConnection>>,
    session_handle: RefCell<Option<String>>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
    // True once the portal acknowledges BindShortcuts
    bound: Cell<bool>,
    // BindShortcuts retries
    bind_attempts: Cell<u32>,
    // CreateSession retries
    create_attempts: Cell<u32>,
    // Guard against subscribing to Activated more than once across retries
    activated_subscribed: Cell<bool>,
}

#[glib::object_subclass]
impl ObjectSubclass for ShortcutsPortalImp {
    const NAME: &'static str = "BridgeShortcutsPortal";
    type Type = ShortcutsPortal;
}

impl ObjectImpl for ShortcutsPortalImp {
    fn signals() -> &'static [Signal] {
        static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| {
            vec![
                Signal::builder("shortcut-activated")
                    .param_types([String::static_type()])
                    .build(),
                Signal::builder("active-changed").build(),
            ]
        })
    }
}

glib::wrapper! {
    pub struct ShortcutsPortal(ObjectSubclass<ShortcutsPortalImp>);
}

impl ShortcutsPortal {
    pub fn new() -> Self {
        glib::Object::new()
    }

    // True once the portal acknowledges BindShortcuts
    pub fn is_active(&self) -> bool {
        self.imp().bound.get()
    }

    pub fn connect_shortcut_activated<F: Fn(&Self, &str) + 'static>(&self, f: F) {
        self.connect_local("shortcut-activated", false, move |args| {
            let portal = args[0].get::<ShortcutsPortal>().unwrap();
            let id = args[1].get::<String>().unwrap();
            f(&portal, &id);
            None
        });
    }

    pub fn connect_active_changed<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("active-changed", false, move |args| {
            let portal = args[0].get::<ShortcutsPortal>().unwrap();
            f(&portal);
            None
        });
    }

    pub fn start(&self, conn: gio::DBusConnection) {
        self.imp().conn.replace(Some(conn.clone()));
        self.imp().create_attempts.set(0);
        self.create_session(&conn);
    }

    fn create_session(&self, conn: &gio::DBusConnection) {
        let attempt = self.imp().create_attempts.get();
        let sender = sender_from_conn(conn);
        let cs_token = format!("bridge_cs_{attempt}");
        let cs_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{cs_token}");
        self.subscribe(
            conn,
            &cs_path,
            REQUEST_IFACE,
            "Response",
            |portal, conn, params| {
                portal.on_create_response(conn, params);
            },
        );

        let mut options: HashMap<String, glib::Variant> = HashMap::new();
        options.insert("handle_token".to_owned(), cs_token.to_variant());
        options.insert(
            "session_handle_token".to_owned(),
            format!("bridge_sh_{attempt}").to_variant(),
        );
        dbus_call(conn, "CreateSession", (options,).to_variant(), "(o)");

        let weak = self.downgrade();
        let conn_c = conn.clone();
        glib::timeout_add_local_once(CREATE_TIMEOUT, move || {
            let Some(portal) = weak.upgrade() else { return };
            let imp = portal.imp();
            if imp.conn.borrow().is_none() || imp.session_handle.borrow().is_some() {
                return;
            }
            portal.retry_create_session(&conn_c);
        });
    }

    fn retry_create_session(&self, conn: &gio::DBusConnection) {
        let attempts = self.imp().create_attempts.get();
        if attempts >= MAX_CREATE_RETRIES {
            eprintln!(
                "GlobalShortcuts giving up after {MAX_CREATE_RETRIES} CreateSession retries; shortcuts inactive"
            );
            self.imp().bound.set(false);
            self.emit_by_name::<()>("active-changed", &[]);
            return;
        }
        let next = attempts + 1;
        self.imp().create_attempts.set(next);
        eprintln!("GlobalShortcuts retrying CreateSession ({next}/{MAX_CREATE_RETRIES})");
        self.create_session(conn);
    }

    pub fn stop(&self) {
        let imp = self.imp();

        imp.subscriptions.borrow_mut().clear();
        {
            let conn = imp.conn.borrow();
            let session = imp.session_handle.borrow();
            if let (Some(conn), Some(session)) = (conn.as_ref(), session.as_deref()) {
                let _ = conn.call_sync(
                    Some(PORTAL_BUS),
                    session,
                    SESSION_IFACE,
                    "Close",
                    None,
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                    None::<&gio::Cancellable>,
                );
            }
        }
        imp.session_handle.replace(None);
        imp.bound.set(false);
        imp.bind_attempts.set(0);
        imp.create_attempts.set(0);
        imp.activated_subscribed.set(false);
        imp.conn.replace(None);
    }

    fn on_create_response(&self, conn: &gio::DBusConnection, params: glib::Variant) {
        let (response, results): (u32, HashMap<String, glib::Variant>) = match params.get() {
            Some(v) => v,
            None => return,
        };
        if response != 0 {
            eprintln!("GlobalShortcuts CreateSession failed (response={response})");
            return;
        }
        let session_handle: String = results
            .get("session_handle")
            .and_then(|v| v.get())
            .unwrap_or_default();
        self.imp()
            .session_handle
            .replace(Some(session_handle.clone()));

        let sender = sender_from_conn(conn);
        let bs_token = "bridge_bs";
        let bs_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{bs_token}");
        self.imp().bind_attempts.set(0);
        self.subscribe(
            conn,
            &bs_path,
            REQUEST_IFACE,
            "Response",
            |portal, conn, params| {
                portal.on_bind_response(conn, params);
            },
        );
        call_bind_shortcuts(conn, &session_handle, bs_token);
    }

    fn on_bind_response(&self, conn: &gio::DBusConnection, params: glib::Variant) {
        let (response, _results): (u32, HashMap<String, glib::Variant>) = match params.get() {
            Some(v) => v,
            None => return,
        };

        if response != 0 {
            eprintln!("GlobalShortcuts BindShortcuts failed (response={response})");
            let attempts = self.imp().bind_attempts.get();
            if attempts < MAX_BIND_RETRIES {
                let next = attempts + 1;
                self.imp().bind_attempts.set(next);
                eprintln!("GlobalShortcuts retrying BindShortcuts ({next}/{MAX_BIND_RETRIES})");
                let weak = self.downgrade();
                let conn_c = conn.clone();
                glib::timeout_add_local_once(BIND_RETRY_BACKOFF, move || {
                    let Some(portal) = weak.upgrade() else { return };
                    let session = portal.imp().session_handle.borrow().clone();
                    let Some(session) = session else { return };
                    let token = format!("bridge_bs_{next}");
                    let bs_path = format!(
                        "/org/freedesktop/portal/desktop/request/{}/{}",
                        sender_from_conn(&conn_c),
                        token,
                    );
                    portal.subscribe(
                        &conn_c,
                        &bs_path,
                        REQUEST_IFACE,
                        "Response",
                        |p, c, params| {
                            p.on_bind_response(c, params);
                        },
                    );
                    call_bind_shortcuts(&conn_c, &session, &token);
                });
                return;
            }
            eprintln!(
                "GlobalShortcuts giving up after {MAX_BIND_RETRIES} retries; shortcuts inactive"
            );
            self.imp().bound.set(false);
            self.emit_by_name::<()>("active-changed", &[]);
            return;
        }

        if !self.imp().activated_subscribed.get() {
            self.subscribe(
                conn,
                PORTAL_PATH,
                SHORTCUTS_IFACE,
                "Activated",
                |portal, _conn, params| {
                    portal.on_activated(params);
                },
            );
            self.imp().activated_subscribed.set(true);
        }

        let session = self
            .imp()
            .session_handle
            .borrow()
            .clone()
            .unwrap_or_default();
        self.imp().bound.set(true);
        eprintln!("Global shortcuts active (session {session})");
        self.emit_by_name::<()>("active-changed", &[]);
    }

    fn on_activated(&self, params: glib::Variant) {
        let (session_handle, shortcut_id, _timestamp, _options): (
            String,
            String,
            u64,
            HashMap<String, glib::Variant>,
        ) = match params.get() {
            Some(v) => v,
            None => return,
        };
        let our_session = self
            .imp()
            .session_handle
            .borrow()
            .clone()
            .unwrap_or_default();
        if session_handle != our_session {
            return;
        }
        self.emit_by_name::<()>("shortcut-activated", &[&shortcut_id]);
    }

    fn subscribe<F>(&self, conn: &gio::DBusConnection, path: &str, iface: &str, signal: &str, f: F)
    where
        F: Fn(&ShortcutsPortal, &gio::DBusConnection, glib::Variant) + 'static,
    {
        let weak = self.downgrade();
        let conn_c = conn.clone();
        let sub = conn.subscribe_to_signal(
            Some(PORTAL_BUS),
            Some(iface),
            Some(signal),
            Some(path),
            None,
            gio::DBusSignalFlags::NONE,
            move |sig| {
                let Some(portal) = weak.upgrade() else { return };
                f(&portal, &conn_c, sig.parameters.clone());
            },
        );
        self.imp().subscriptions.borrow_mut().push(sub);
    }

    pub fn list_shortcuts<F>(&self, f: F)
    where
        F: FnOnce(Vec<(String, String, String)>) + 'static,
    {
        let conn_opt = self.imp().conn.borrow().clone();
        let session_opt = self.imp().session_handle.borrow().clone();

        let (conn, session) = match (conn_opt, session_opt) {
            (Some(c), Some(s)) => (c, s),
            _ => {
                let inactive = SHORTCUTS
                    .iter()
                    .map(|(id, desc, _)| (id.to_string(), desc.to_string(), String::new()))
                    .collect();
                f(inactive);
                return;
            }
        };

        let sender = sender_from_conn(&conn);
        let token = "bridge_ls";
        let ls_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");

        let f_cell = Rc::new(RefCell::new(Some(f)));

        // Holds the sub guard so it can be dropped after the first fire
        let sub_ref: Rc<RefCell<Option<gio::SignalSubscription>>> = Rc::new(RefCell::new(None));
        let sub_ref_c = Rc::clone(&sub_ref);

        let sub = conn.subscribe_to_signal(
            Some(PORTAL_BUS),
            Some(REQUEST_IFACE),
            Some("Response"),
            Some(ls_path.as_str()),
            None,
            gio::DBusSignalFlags::NONE,
            move |sig| {
                sub_ref_c.borrow_mut().take();

                let (response, results): (u32, HashMap<String, glib::Variant>) =
                    match sig.parameters.get() {
                        Some(v) => v,
                        None => return,
                    };
                if response != 0 {
                    return;
                }

                let list: Vec<(String, String, String)> = results
                    .get("shortcuts")
                    .map(|v| {
                        let n = v.n_children();
                        let mut out = Vec::with_capacity(n);
                        for i in 0..n {
                            let child = v.child_value(i);
                            if let Some((id, props)) =
                                child.get::<(String, HashMap<String, glib::Variant>)>()
                            {
                                let desc = props
                                    .get("description")
                                    .and_then(|p| p.get::<String>())
                                    .unwrap_or_default();
                                let trigger = props
                                    .get("trigger_description")
                                    .and_then(|p| p.get::<String>())
                                    .unwrap_or_default();
                                out.push((id, desc, trigger));
                            }
                        }
                        out
                    })
                    .unwrap_or_default();

                if let Some(cb) = f_cell.borrow_mut().take() {
                    cb(list);
                }
            },
        );
        *sub_ref.borrow_mut() = Some(sub);

        let mut opts: HashMap<String, glib::Variant> = HashMap::new();
        opts.insert("handle_token".to_owned(), token.to_variant());
        let session_path = match glib::variant::ObjectPath::try_from(session) {
            Ok(p) => p,
            Err(_) => {
                eprintln!("list_shortcuts: invalid session path");
                return;
            }
        };
        dbus_call(
            &conn,
            "ListShortcuts",
            (session_path, opts).to_variant(),
            "(o)",
        );
    }
}

fn sender_from_conn(conn: &gio::DBusConnection) -> String {
    conn.unique_name()
        .map(|n| n.to_string())
        .unwrap_or_default()
        .trim_start_matches(':')
        .replace('.', "_")
}

fn dbus_call(conn: &gio::DBusConnection, method: &str, params: glib::Variant, reply_type: &str) {
    // result comes from the subscribed response signal, not the reply
    let method_owned = method.to_owned();
    conn.call(
        Some(PORTAL_BUS),
        PORTAL_PATH,
        SHORTCUTS_IFACE,
        method,
        Some(&params),
        Some(glib::VariantTy::new(reply_type).unwrap()),
        gio::DBusCallFlags::NONE,
        -1,
        None::<&gio::Cancellable>,
        move |res| {
            if let Err(e) = res {
                eprintln!("dbus call {method_owned} failed: {e}");
            }
        },
    );
}

fn call_bind_shortcuts(conn: &gio::DBusConnection, session_handle: &str, token: &str) {
    let shortcuts: Vec<(String, HashMap<String, glib::Variant>)> = SHORTCUTS
        .iter()
        .map(|(id, desc, trigger)| {
            let mut props = HashMap::new();
            props.insert("description".to_owned(), desc.to_variant());
            props.insert("preferred_trigger".to_owned(), trigger.to_variant());
            (id.to_string(), props)
        })
        .collect();

    let mut bind_opts: HashMap<String, glib::Variant> = HashMap::new();
    bind_opts.insert("handle_token".to_owned(), token.to_variant());

    let session_path = match glib::variant::ObjectPath::try_from(session_handle.to_owned()) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("call_bind_shortcuts: invalid session path");
            return;
        }
    };
    let params = (session_path, shortcuts, String::new(), bind_opts).to_variant();
    dbus_call(conn, "BindShortcuts", params, "(o)");
}
