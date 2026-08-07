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

// Response code for a user cancel
const RESPONSE_CANCELLED: u32 = 1;

const MAX_HANDSHAKE_ATTEMPTS: u32 = 4;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_BACKOFF: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct ShortcutsPortalImp {
    conn: RefCell<Option<gio::DBusConnection>>,
    session_handle: RefCell<Option<String>>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
    activated_sub: RefCell<Option<gio::SignalSubscription>>,
    bound: Cell<bool>,
    awaiting_bind: Cell<bool>,
    handshaking: Cell<bool>,
    attempt: Cell<u32>,
    generation: Cell<u64>,
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

    // True once the shortcuts are bound
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
        self.imp().conn.replace(Some(conn));
        self.imp().attempt.set(0);
        self.begin_handshake();
    }

    // Destroys the existing session to begin a new portal handshake
    pub fn restart(&self) {
        if self.imp().conn.borrow().is_none() {
            return;
        }
        self.close_session();
        self.imp().attempt.set(0);
        self.begin_handshake();
    }

    fn begin_handshake(&self) {
        let conn = self.imp().conn.borrow().clone();
        let Some(conn) = conn else { return };

        let attempt = self.imp().attempt.get();
        let generation = self.bump_generation();
        self.imp().handshaking.set(true);
        let sender = sender_from_conn(&conn);
        let cs_token = format!("bridge_cs_{attempt}");
        let cs_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{cs_token}");
        self.subscribe(
            &conn,
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
        dbus_call(&conn, "CreateSession", (options,).to_variant(), "(o)");

        let weak = self.downgrade();
        glib::timeout_add_local_once(HANDSHAKE_TIMEOUT, move || {
            let Some(portal) = weak.upgrade() else { return };
            let imp = portal.imp();
            if imp.bound.get() || imp.awaiting_bind.get() || portal.is_stale(generation) {
                return;
            }
            eprintln!("GlobalShortcuts handshake timed out - attempt {attempt}");
            portal.retry_handshake();
        });
    }

    fn give_up(&self) {
        self.close_session();
        let imp = self.imp();
        imp.attempt.set(MAX_HANDSHAKE_ATTEMPTS);
        imp.handshaking.set(false);
        self.emit_by_name::<()>("active-changed", &[]);
    }

    // true while still negociating the portal chain
    pub fn is_handshaking(&self) -> bool {
        self.imp().handshaking.get()
    }

    fn retry_handshake(&self) {
        let imp = self.imp();
        let next = imp.attempt.get() + 1;
        if next >= MAX_HANDSHAKE_ATTEMPTS {
            eprintln!(
                "GlobalShortcuts giving up after {MAX_HANDSHAKE_ATTEMPTS} attempts; shortcuts inactive"
            );
            self.give_up();
            return;
        }
        imp.attempt.set(next);
        self.close_session();
        let generation = imp.generation.get();

        let backoff = RETRY_BACKOFF * 2u32.pow(next - 1);
        eprintln!("GlobalShortcuts retrying handshake - {next}/{MAX_HANDSHAKE_ATTEMPTS}");
        let weak = self.downgrade();
        glib::timeout_add_local_once(backoff, move || {
            let Some(portal) = weak.upgrade() else { return };
            if portal.is_stale(generation) {
                return;
            }
            portal.begin_handshake();
        });
    }

    // generation for every attempt
    fn bump_generation(&self) -> u64 {
        let next = self.imp().generation.get() + 1;
        self.imp().generation.set(next);
        next
    }

    fn is_stale(&self, generation: u64) -> bool {
        let imp = self.imp();
        imp.conn.borrow().is_none() || imp.generation.get() != generation
    }

    // drops subs and closes the session
    // clean session for next attempt
    fn close_session(&self) {
        let imp = self.imp();
        imp.subscriptions.borrow_mut().clear();
        imp.awaiting_bind.set(false);
        let was_bound = imp.bound.replace(false);
        self.bump_generation();

        let conn = imp.conn.borrow().clone();
        let session = imp.session_handle.take();
        if let (Some(conn), Some(session)) = (conn, session) {
            conn.call(
                Some(PORTAL_BUS),
                &session,
                SESSION_IFACE,
                "Close",
                None,
                None,
                gio::DBusCallFlags::NONE,
                -1,
                None::<&gio::Cancellable>,
                |_| {},
            );
        }

        if was_bound {
            self.emit_by_name::<()>("active-changed", &[]);
        }
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
        imp.awaiting_bind.set(false);
        imp.handshaking.set(false);
        imp.attempt.set(0);
        imp.activated_sub.replace(None);
        imp.conn.replace(None);
    }

    fn on_create_response(&self, conn: &gio::DBusConnection, params: glib::Variant) {
        let (response, results): (u32, HashMap<String, glib::Variant>) = match params.get() {
            Some(v) => v,
            None => return,
        };

        if self.imp().session_handle.borrow().is_some() {
            return;
        }
        if response != 0 {
            eprintln!("GlobalShortcuts CreateSession failed (response={response})");
            self.retry_handshake();
            return;
        }
        let session_handle: String = results
            .get("session_handle")
            .and_then(|v| v.get())
            .unwrap_or_default();
        if session_handle.is_empty() {
            eprintln!("GlobalShortcuts CreateSession returned no session handle");
            self.retry_handshake();
            return;
        }
        self.imp()
            .session_handle
            .replace(Some(session_handle.clone()));

        // only one bind attempt per session
        let attempt = self.imp().attempt.get();
        let generation = self.imp().generation.get();
        let weak = self.downgrade();
        let conn_c = conn.clone();
        self.request_shortcut_list(
            conn,
            &session_handle,
            &format!("bridge_hs_{attempt}"),
            move |list| {
                let Some(portal) = weak.upgrade() else { return };
                if portal.is_stale(generation) {
                    return;
                }
                match list {
                    Some(list) if !list.is_empty() => portal.finish_bound(&conn_c),
                    _ => portal.send_bind(&conn_c),
                }
            },
        );
    }

    fn send_bind(&self, conn: &gio::DBusConnection) {
        let session = self
            .imp()
            .session_handle
            .borrow()
            .clone()
            .unwrap_or_default();
        if session.is_empty() {
            return;
        }
        let attempt = self.imp().attempt.get();
        let token = format!("bridge_bs_{attempt}");
        let path = format!(
            "/org/freedesktop/portal/desktop/request/{}/{}",
            sender_from_conn(conn),
            token,
        );
        self.subscribe(
            conn,
            &path,
            REQUEST_IFACE,
            "Response",
            |portal, conn, params| {
                portal.on_bind_response(conn, params);
            },
        );
        self.imp().awaiting_bind.set(true);
        call_bind_shortcuts(conn, &session, &token);
    }

    fn on_bind_response(&self, conn: &gio::DBusConnection, params: glib::Variant) {
        let (response, _results): (u32, HashMap<String, glib::Variant>) = match params.get() {
            Some(v) => v,
            None => return,
        };
        self.imp().awaiting_bind.set(false);

        if response == RESPONSE_CANCELLED {
            eprintln!("GlobalShortcuts BindShortcuts cancelled by the user");
            self.give_up();
            return;
        }
        if response != 0 {
            eprintln!("GlobalShortcuts BindShortcuts failed (response={response})");
            self.retry_handshake();
            return;
        }
        self.finish_bound(conn);
    }

    fn finish_bound(&self, conn: &gio::DBusConnection) {
        let imp = self.imp();
        if imp.bound.get() {
            return;
        }

        if imp.activated_sub.borrow().is_none() {
            let sub = self.subscribe_raw(
                conn,
                PORTAL_PATH,
                SHORTCUTS_IFACE,
                "Activated",
                |portal, _conn, params| {
                    portal.on_activated(params);
                },
            );
            imp.activated_sub.replace(Some(sub));
        }

        let session = imp.session_handle.borrow().clone().unwrap_or_default();
        imp.bound.set(true);
        imp.handshaking.set(false);
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

    fn subscribe_raw<F>(
        &self,
        conn: &gio::DBusConnection,
        path: &str,
        iface: &str,
        signal: &str,
        f: F,
    ) -> gio::SignalSubscription
    where
        F: Fn(&ShortcutsPortal, &gio::DBusConnection, glib::Variant) + 'static,
    {
        let weak = self.downgrade();
        let conn_c = conn.clone();
        conn.subscribe_to_signal(
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
        )
    }

    fn subscribe<F>(&self, conn: &gio::DBusConnection, path: &str, iface: &str, signal: &str, f: F)
    where
        F: Fn(&ShortcutsPortal, &gio::DBusConnection, glib::Variant) + 'static,
    {
        let sub = self.subscribe_raw(conn, path, iface, signal, f);
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
                f(untriggered_shortcuts());
                return;
            }
        };

        self.request_shortcut_list(&conn, &session, "bridge_ls", move |list| {
            f(list.unwrap_or_else(untriggered_shortcuts))
        });
    }

    fn request_shortcut_list<F>(&self, conn: &gio::DBusConnection, session: &str, token: &str, f: F)
    where
        F: FnOnce(Option<Vec<(String, String, String)>>) + 'static,
    {
        let sender = sender_from_conn(conn);
        let ls_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");

        let f_cell = Rc::new(RefCell::new(Some(f)));
        let f_cell_ret = Rc::clone(&f_cell);

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
                    if let Some(cb) = f_cell.borrow_mut().take() {
                        cb(None);
                    }
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
                    cb(Some(list));
                }
            },
        );
        *sub_ref.borrow_mut() = Some(sub);

        let mut opts: HashMap<String, glib::Variant> = HashMap::new();
        opts.insert("handle_token".to_owned(), token.to_variant());
        let session_path = match glib::variant::ObjectPath::try_from(session.to_owned()) {
            Ok(p) => p,
            Err(_) => {
                eprintln!("list_shortcuts: invalid session path");
                sub_ref.borrow_mut().take();
                if let Some(cb) = f_cell_ret.borrow_mut().take() {
                    cb(None);
                }
                return;
            }
        };
        dbus_call(
            conn,
            "ListShortcuts",
            (session_path, opts).to_variant(),
            "(o)",
        );
    }
}

fn untriggered_shortcuts() -> Vec<(String, String, String)> {
    SHORTCUTS
        .iter()
        .map(|(id, desc, _)| (id.to_string(), desc.to_string(), String::new()))
        .collect()
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
