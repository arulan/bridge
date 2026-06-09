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
use std::rc::Rc;

use crate::wp;
use super::hw_sink::{HwSink, hw_sink_from_node};

pub struct PipeWireBackend {
    sinks: RefCell<HashMap<u32, HwSink>>,

    // Order is important: om before core
    om:    RefCell<Option<wp::ObjectManager>>,
    core:  RefCell<Option<wp::Core>>,
}

impl PipeWireBackend {
    pub fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            sinks: RefCell::new(HashMap::new()),
            om:    RefCell::new(None),
            core:  RefCell::new(None),
        }))
    }

    pub fn hw_sinks(&self) -> Vec<HwSink> {
        self.sinks.borrow().values().cloned().collect()
    }
}

pub fn start(backend: &Rc<RefCell<PipeWireBackend>>) {
    wp::init_all();
    let core = wp::Core::new();
    let om = wp::ObjectManager::new();

    let be = Rc::clone(backend);
    om.connect_object_added(move |obj| {
        if let Some(sink) = hw_sink_from_node(&obj) {
            be.borrow().sinks.borrow_mut().insert(sink.node_id, sink);
        }
    });

    let be = Rc::clone(backend);
    om.connect_object_removed(move |obj| {
        let id = wp::bound_id(&obj);
        be.borrow().sinks.borrow_mut().remove(&id);
    });

    om.add_interest_for_type(wp::node_type());
    om.request_object_features(wp::node_type(), wp::WP_PIPEWIRE_OBJECT_FEATURE_INFO);
    core.install_object_manager(&om);
    core.connect();

    backend.borrow().core.replace(Some(core));
    backend.borrow().om.replace(Some(om));
}

pub fn stop(backend: &Rc<RefCell<PipeWireBackend>>) {
    let b = backend.borrow();
    if let Some(core) = b.core.borrow().as_ref() {
        core.disconnect();
    }
    // Teardown; Order is important: om before core
    b.om.replace(None);
    b.core.replace(None);
}
