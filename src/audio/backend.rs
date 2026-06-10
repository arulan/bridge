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
use std::collections::HashMap;
use std::sync::OnceLock;

use glib::prelude::*;
use glib::subclass::prelude::*;
use glib::subclass::Signal;

use crate::wp;
use super::hw_sink::{HwSink, hw_sink_from_node};

#[derive(Default)]
pub struct PipeWireBackendImp {
    sinks: RefCell<HashMap<u32, HwSink>>,

    // Our own loopback capture nodes
    owned_roles: RefCell<HashMap<u32, String>>,

    // Order is important: om before core
    om:    RefCell<Option<wp::ObjectManager>>,
    core:  RefCell<Option<wp::Core>>,
}

#[glib::object_subclass]
impl ObjectSubclass for PipeWireBackendImp {
    const NAME: &'static str = "DashboardPipeWireBackend";
    type Type = PipeWireBackend;
}

impl ObjectImpl for PipeWireBackendImp {
    fn signals() -> &'static [Signal] {
        static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| {
            vec![Signal::builder("sinks-ready").build()]
        })
    }
}

glib::wrapper! {
    pub struct PipeWireBackend(ObjectSubclass<PipeWireBackendImp>);
}

impl PipeWireBackend {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn start(&self) {
        wp::init_all();
        let core = wp::Core::new();
        let om = wp::ObjectManager::new();

        om.connect_object_added(glib::clone!(
            #[weak(rename_to = be)] self,
            move |obj| be.on_object_added(obj)
        ));

        om.connect_object_removed(glib::clone!(
            #[weak(rename_to = be)] self,
            move |obj| be.on_object_removed(obj)
        ));

        om.connect_installed(glib::clone!(
            #[weak(rename_to = be)] self,
            move || be.emit_by_name::<()>("sinks-ready", &[])
        ));

        om.add_interest_for_type(wp::node_type());
        om.request_object_features(wp::node_type(), wp::WP_PIPEWIRE_OBJECT_FEATURE_INFO);
        core.install_object_manager(&om);
        core.connect();

        let imp = self.imp();
        imp.core.replace(Some(core));
        imp.om.replace(Some(om));
    }

    pub fn stop(&self) {
        let imp = self.imp();
        if let Some(core) = imp.core.borrow().as_ref() {
            core.disconnect();
        }
        // Teardown; Order is important: om before core
        imp.om.replace(None);
        imp.core.replace(None);
    }

    /// sorted hardware sinks
    pub fn hw_sinks(&self) -> Vec<HwSink> {
        let mut sinks: Vec<HwSink> = self.imp().sinks.borrow().values().cloned().collect();
        sinks.sort_by_key(|s| s.display_name.to_lowercase());
        sinks
    }

    pub fn owned_sinks_present(&self) -> bool {
        let roles = self.imp().owned_roles.borrow();
        ["aux", "main"].iter().all(|want| roles.values().any(|have| have == want))
    }

    pub fn connect_sinks_ready<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("sinks-ready", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }

    fn on_object_added(&self, obj: glib::Object) {
        if let Some(role) = wp::node_prop(&obj, "dashboard.role") {
            self.imp().owned_roles.borrow_mut().insert(wp::bound_id(&obj), role);
            return;
        }
        if let Some(sink) = hw_sink_from_node(&obj) {
            self.imp().sinks.borrow_mut().insert(sink.node_id, sink);
        }
    }

    fn on_object_removed(&self, obj: glib::Object) {
        let id = wp::bound_id(&obj);
        self.imp().sinks.borrow_mut().remove(&id);
        self.imp().owned_roles.borrow_mut().remove(&id);
    }
}
