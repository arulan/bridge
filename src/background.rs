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
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use gio::prelude::*;

const PORTAL_BUS: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const BACKGROUND_IFACE: &str = "org.freedesktop.portal.Background";
const REQUEST_IFACE: &str = "org.freedesktop.portal.Request";

#[derive(Clone, Copy, Debug)]
pub struct BackgroundGrant {
    pub background: bool,
    pub autostart: bool,
}

// Background Portal request
pub fn request_background<F: Fn(BackgroundGrant) + 'static>(
    conn: &gio::DBusConnection,
    reason: &str,
    autostart: Option<&[&str]>,
    on_result: F,
) {
    // fresh token per call
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let sender = portal_sender_token(conn);
    let token = format!("bridge_bg_{}", COUNTER.fetch_add(1, Ordering::Relaxed));
    let req_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");

    // sub guard
    let sub_ref: Rc<RefCell<Option<gio::SignalSubscription>>> = Rc::new(RefCell::new(None));
    let sub_ref_c = Rc::clone(&sub_ref);
    let sub = conn.subscribe_to_signal(
        Some(PORTAL_BUS),
        Some(REQUEST_IFACE),
        Some("Response"),
        Some(req_path.as_str()),
        None,
        gio::DBusSignalFlags::NONE,
        move |sig| {
            sub_ref_c.borrow_mut().take();

            let grant = match sig
                .parameters
                .get::<(u32, HashMap<String, glib::Variant>)>()
            {
                Some((0, results)) => {
                    let get = |key| {
                        results
                            .get(key)
                            .and_then(|v: &glib::Variant| v.get::<bool>())
                            .unwrap_or(false)
                    };
                    BackgroundGrant {
                        background: get("background"),
                        autostart: get("autostart"),
                    }
                }
                _ => BackgroundGrant {
                    background: false,
                    autostart: false,
                },
            };

            on_result(grant);
        },
    );
    *sub_ref.borrow_mut() = Some(sub);

    let mut options: HashMap<String, glib::Variant> = HashMap::new();
    options.insert("handle_token".to_owned(), token.to_variant());
    options.insert("reason".to_owned(), reason.to_variant());
    options.insert("autostart".to_owned(), autostart.is_some().to_variant());
    if let Some(argv) = autostart {
        options.insert("commandline".to_owned(), argv.to_variant());
    }

    let params = ("", options).to_variant();
    conn.call(
        Some(PORTAL_BUS),
        PORTAL_PATH,
        BACKGROUND_IFACE,
        "RequestBackground",
        Some(&params),
        Some(glib::VariantTy::new("(o)").unwrap()),
        gio::DBusCallFlags::NONE,
        -1,
        None::<&gio::Cancellable>,
        |res| {
            if let Err(e) = res {
                eprintln!("RequestBackground call failed: {e}");
            }
        },
    );
}

fn portal_sender_token(conn: &gio::DBusConnection) -> String {
    conn.unique_name()
        .map(|n| n.to_string())
        .unwrap_or_default()
        .trim_start_matches(':')
        .replace('.', "_")
}
