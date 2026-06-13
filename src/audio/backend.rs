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

use crate::config::Side;
use crate::wp;
use super::hw_sink::{HwSink, hw_sink_from_node};
use super::level_meter::LevelMeters;
use super::pw_config;
use super::test_tone;

struct OwnedNode {
    id:   u32,
    node: wp::Node,
}

#[derive(Default)]
pub struct PipeWireBackendImp {
    sinks: RefCell<HashMap<u32, HwSink>>,

    // Our own loopback capture nodes, keyed by Side
    owned: RefCell<HashMap<Side, OwnedNode>>,

    // Our session-only loopback modules; Empty once persistent sinks are created
    modules: RefCell<HashMap<Side, wp::LoadedModule>>,

    default_metadata: RefCell<Option<wp::Metadata>>,

    // Per-sink level meters
    level_meters: RefCell<Option<LevelMeters>>,

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
            vec![
                Signal::builder("sinks-ready").build(),
                Signal::builder("default-changed").build(),
                Signal::builder("owned-changed").build(),
            ]
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
            move || {
                // relaunch the app, but persistent sinks aren't live yet, such as
                // after first-run setup -> close app -> relaunch app.
                // In this case, we create the temp sinks again.
                be.create_missing_temp_sinks();
                be.emit_by_name::<()>("sinks-ready", &[]);
            }
        ));

        om.add_interest_for_type(wp::node_type());
        om.request_object_features(wp::node_type(), wp::WP_PIPEWIRE_OBJECT_FEATURE_INFO);
   
        om.add_interest_for_type(wp::metadata_type());
        om.request_object_features(wp::metadata_type(), wp::WP_PROXY_FEATURE_BOUND);
        core.install_object_manager(&om);
        core.connect();

        let imp = self.imp();
        imp.core.replace(Some(core));
        imp.om.replace(Some(om));

        imp.level_meters.replace(Some(LevelMeters::start()));
    }

    pub fn stop(&self) {
        let imp = self.imp();

        // return sinks to 100% volume and unmuted when closing Dashboard
        // TODO: Revisit design choice & possible preferences setting
        for owned in imp.owned.borrow().values() {
            owned.node.set_mute(false);
            owned.node.set_volume(1.0);
        }

        // Teardown; Order is important: modules -> om -> core
        imp.modules.borrow_mut().clear();
        if let Some(core) = imp.core.borrow().as_ref() {
            core.disconnect();
        }
        // Teardown; Order is important: modules -> om -> core
        imp.om.replace(None);
        imp.core.replace(None);
    }

    /// Sorted hardware sinks
    pub fn hw_sinks(&self) -> Vec<HwSink> {
        let mut sinks: Vec<HwSink> = self.imp().sinks.borrow().values().cloned().collect();
        sinks.sort_by_key(|s| s.display_name.to_lowercase());
        sinks
    }

    pub fn owned_sinks_present(&self) -> bool {
        let owned = self.imp().owned.borrow();
        [Side::Aux, Side::Main].iter().all(|side| owned.contains_key(side))
    }

    /// True while sink is a session-only loopback we loaded, rather than a
    /// persistent one from the conf
    pub fn using_temp_sinks(&self) -> bool {
        !self.imp().modules.borrow().is_empty()
    }

    /// Create in-process loopback for any configured side that isn't
    /// already live with a persistent sink
    pub fn create_missing_temp_sinks(&self) {
        if !crate::config::is_configured() {
            return;
        }
        let cfg = crate::config::load();
        let imp = self.imp();

        for side in [Side::Aux, Side::Main] {
            if imp.owned.borrow().contains_key(&side) || imp.modules.borrow().contains_key(&side) {
                continue;
            }
            let args = pw_config::loopback_module_args(side, cfg.side(side));
            let module = imp.core.borrow().as_ref()
                .and_then(|core| core.load_module("libpipewire-module-loopback", &args));
            match module {
                Some(m) => { imp.modules.borrow_mut().insert(side, m); }
                None    => eprintln!("backend: failed to load temp loopback for {side:?}"),
            }
        }
    }

    /// Clear our loopbacks and recreate them for the current config
    /// Used when running Set Up again
    pub fn recreate_temp_sinks(&self) {
        self.imp().modules.borrow_mut().clear();
        self.create_missing_temp_sinks();
    }

    /// Sets the volume on one of our sinks
    pub fn set_volume(&self, side: Side, volume: f64) {
        if let Some(owned) = self.imp().owned.borrow().get(&side) {
            owned.node.set_volume(volume as f32);
        }
    }

    /// Mutes or unmutes one of our sinks
    pub fn set_mute(&self, side: Side, muted: bool) {
        if let Some(owned) = self.imp().owned.borrow().get(&side) {
            owned.node.set_mute(muted);
        }
    }

    /// Play per-channel test tone through our virtual sinks. The layout comes
    /// from the saved config.
    pub fn play_test_tone(&self, side: Side, on_done: impl FnOnce() + Send + 'static) {
        let sink_name = match side {
            Side::Aux  => pw_config::AUX_SINK,
            Side::Main => pw_config::MAIN_SINK,
        };

        let def = crate::config::load();
        let def = def.side(side);
        let n_channels = def.channels.max(2);
        let positions  = test_tone::pos_str_to_spa_ids(&def.position, n_channels);
        let sweep      = (0..positions.len()).collect();

        test_tone::play_through_sink(sink_name, n_channels, positions, sweep, on_done);
    }

  
    /// Get the latest peak level on each side's sink
    pub fn peak(&self, side: Side) -> f32 {
        self.imp().level_meters.borrow().as_ref().map_or(0.0, |m| m.peak(side))
    }

    pub fn set_main_default(&self) {
        if let Some(meta) = self.imp().default_metadata.borrow().as_ref() {
            meta.set_default_sink(pw_config::MAIN_SINK);
        }
    }

    /// node.name of the current system default sink
    pub fn default_sink_name(&self) -> Option<String> {
        self.imp().default_metadata.borrow().as_ref()
            .and_then(|meta| meta.find(0, "default.audio.sink"))
            .and_then(|v| crate::util::parse_default_name(&v))
    }

    pub fn main_is_default(&self) -> Option<bool> {
        self.default_sink_name().map(|name| name == pw_config::MAIN_SINK)
    }

    pub fn connect_sinks_ready<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("sinks-ready", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }

    pub fn connect_default_changed<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("default-changed", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }

    pub fn connect_owned_changed<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("owned-changed", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }
    

    fn on_object_added(&self, obj: glib::Object) {
  
        // We're only concerned with the default metadata obj
        if let Some(meta_name) = wp::node_prop(&obj, "metadata.name") {
            if meta_name == "default" {
                let meta = wp::Metadata::from_object(obj);

                // Emit when default changes externally
                meta.connect_changed(glib::clone!(
                    #[weak(rename_to = be)] self,
                    move |subject, key, _value| {
                        if subject == 0 && key.as_deref() == Some("default.audio.sink") {
                            be.emit_by_name::<()>("default-changed", &[]);
                        }
                    }
                ));

                meta.activate_data(glib::clone!(
                    #[weak(rename_to = be)] self,
                    move |_ok| be.emit_by_name::<()>("default-changed", &[])
                ));

                self.imp().default_metadata.borrow_mut().replace(meta);
            }
            return;
        }

        // role is only in the full info props
        if let Some(role) = wp::node_pw_prop(&obj, "dashboard.role") {
            if let Some(side) = Side::from_wire(&role) {
                let id = wp::bound_id(&obj);
                let node = wp::Node::from_object(obj);
                self.imp().owned.borrow_mut().insert(side, OwnedNode { id, node });
                self.emit_by_name::<()>("owned-changed", &[]);
            }
            return;
        }
        if let Some(sink) = hw_sink_from_node(&obj) {
            self.imp().sinks.borrow_mut().insert(sink.node_id, sink);
        }
    }

    fn on_object_removed(&self, obj: glib::Object) {
        let id = wp::bound_id(&obj);
        self.imp().sinks.borrow_mut().remove(&id);

        let owned_dropped = {
            let mut owned = self.imp().owned.borrow_mut();
            let before = owned.len();
            owned.retain(|_, owned| owned.id != id);
            owned.len() != before
        };

        if owned_dropped {
            self.emit_by_name::<()>("owned-changed", &[]);
        }
    }
}
