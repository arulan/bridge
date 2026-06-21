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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::OnceLock;

use glib::prelude::*;
use glib::subclass::Signal;
use glib::subclass::prelude::*;

use super::hw_sink::HwSink;
use super::level_meter::LevelMeters;
use super::pw_config;
use super::pw_connection::{Event, PwConnection, Request};
use super::test_tone;
use crate::config::{self, Side};

#[derive(Default)]
pub struct PipeWireBackendImp {
    // Mirrors the pw side state
    sinks: RefCell<HashMap<u32, HwSink>>,
    owned: RefCell<HashMap<Side, u32>>,
    default_name: RefCell<Option<String>>,

    // gate sinks-ready vs sinks-changed
    installed: Cell<bool>,

    using_temp: Cell<bool>,

    level_meters: RefCell<Option<LevelMeters>>,
    pw: RefCell<Option<PwConnection>>,
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
                Signal::builder("sinks-changed").build(),
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
        let meters = LevelMeters::new();
        let (aux_peak, main_peak) = meters.atoms();
        self.imp().level_meters.replace(Some(meters));

        let (pw, evt_rx) = PwConnection::start(aux_peak, main_peak);
        self.imp().pw.replace(Some(pw));

        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            while let Ok(evt) = evt_rx.recv().await {
                let Some(be) = weak.upgrade() else { break };
                be.handle_event(evt);
            }
        });
    }

    pub fn stop(&self) {
        let imp = self.imp();

        // The pw thread returns our sinks to 1.0 volume, unmutes, and flushes
        if let Some(pw) = imp.pw.borrow().as_ref() {
            pw.shutdown();
        }
        imp.pw.replace(None);
        imp.level_meters.replace(None);
    }

    fn handle_event(&self, evt: Event) {
        let imp = self.imp();
        match evt {
            Event::Settled => {
                self.create_missing_temp_sinks();
                imp.installed.set(true);
                self.emit_by_name::<()>("sinks-ready", &[]);
            }
            Event::SinkAdded(sink) => {
                imp.sinks.borrow_mut().insert(sink.node_id, sink);
                if imp.installed.get() {
                    self.emit_by_name::<()>("sinks-changed", &[]);
                }
            }
            Event::SinkRemoved(id) => {
                let dropped = imp.sinks.borrow_mut().remove(&id).is_some();
                if dropped && imp.installed.get() {
                    self.emit_by_name::<()>("sinks-changed", &[]);
                }
            }
            Event::OwnedAdded { side, id } => {
                imp.owned.borrow_mut().insert(side, id);
                self.emit_by_name::<()>("owned-changed", &[]);
            }
            Event::OwnedRemoved { side } => {
                if imp.owned.borrow_mut().remove(&side).is_some() {
                    self.emit_by_name::<()>("owned-changed", &[]);
                }
            }
            Event::DefaultSink(raw) => {
                let name = raw.and_then(|v| crate::util::parse_default_name(&v));
                imp.default_name.replace(name);
                self.emit_by_name::<()>("default-changed", &[]);
            }
        }
    }

    /// Sorted hardware sinks
    pub fn hw_sinks(&self) -> Vec<HwSink> {
        let mut sinks: Vec<HwSink> = self.imp().sinks.borrow().values().cloned().collect();
        sinks.sort_by_key(|s| s.display_name.to_lowercase());
        sinks
    }

    pub fn owned_sinks_present(&self) -> bool {
        let owned = self.imp().owned.borrow();
        [Side::Aux, Side::Main]
            .iter()
            .all(|side| owned.contains_key(side))
    }

    /// True while sink is a session-only loopback we loaded, rather than a
    /// persistent one from the conf
    pub fn using_temp_sinks(&self) -> bool {
        self.imp().using_temp.get()
    }

    fn temp_sink_configs(&self) -> Vec<(Side, String)> {
        if !config::is_configured() {
            return Vec::new();
        }
        let cfg = config::load();
        let owned = self.imp().owned.borrow();
        [Side::Aux, Side::Main]
            .into_iter()
            .filter(|side| !owned.contains_key(side))
            .map(|side| (side, pw_config::loopback_module_args(side, cfg.side(side))))
            .collect()
    }

    /// Create in-process loopback for any configured side that isn't
    /// already live with a persistent sink
    pub fn create_missing_temp_sinks(&self) {
        let configs = self.temp_sink_configs();
        self.imp().using_temp.set(!configs.is_empty());
        if configs.is_empty() {
            return;
        }
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::CreateTempSinks(configs));
        }
    }

    /// Clear our loopbacks and recreate them for the current config
    /// Used when running Set Up again
    pub fn recreate_temp_sinks(&self) {
        let configs = self.temp_sink_configs();
        self.imp().using_temp.set(!configs.is_empty());
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::RecreateTempSinks(configs));
        }
    }

    /// Live routing of one side's (Aux or Main) hardware output by node.name
    /// None targets the system default; The conf writes the target for new sessions
    pub fn retarget(&self, side: Side, hw_name: &str) {
        let hw_name = (!hw_name.is_empty()).then(|| hw_name.to_owned());
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::Retarget { side, hw_name });
        }
    }

    /// Sets the volume on one of our sinks
    pub fn set_volume(&self, side: Side, volume: f64) {
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::SetVolume {
                side,
                volume: volume as f32,
            });
        }
    }

    /// Mutes or unmutes one of our sinks
    pub fn set_mute(&self, side: Side, muted: bool) {
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::SetMute { side, muted });
        }
    }

    /// Play per-channel test tone through our virtual sinks. The layout comes
    /// from the saved config.
    pub fn play_test_tone(&self, side: Side, on_done: impl FnOnce() + Send + 'static) {
        let sink_name = match side {
            Side::Aux => pw_config::AUX_SINK,
            Side::Main => pw_config::MAIN_SINK,
        };

        let def = config::load();
        let def = def.side(side);
        let n_channels = def.channels.max(2);
        let positions = test_tone::pos_str_to_spa_ids(&def.position, n_channels);
        let sweep = (0..positions.len()).collect();

        test_tone::play_through_sink(sink_name, n_channels, positions, sweep, on_done);
    }

    /// Get the latest peak level on each side's sink
    pub fn peak(&self, side: Side) -> f32 {
        self.imp()
            .level_meters
            .borrow()
            .as_ref()
            .map_or(0.0, |m| m.peak(side))
    }

    pub fn set_main_default(&self) {
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::SetDefault(pw_config::MAIN_SINK.to_owned()));
        }
    }

    /// node.name of the current system default sink
    pub fn default_sink_name(&self) -> Option<String> {
        self.imp().default_name.borrow().clone()
    }

    pub fn main_is_default(&self) -> Option<bool> {
        self.default_sink_name()
            .map(|name| name == pw_config::MAIN_SINK)
    }

    pub fn connect_sinks_ready<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("sinks-ready", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }

    pub fn connect_sinks_changed<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("sinks-changed", false, move |args| {
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
}
