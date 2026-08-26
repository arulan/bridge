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
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, OnceLock};

use glib::prelude::*;
use glib::subclass::Signal;
use glib::subclass::prelude::*;

use super::hw_device::{self, HwDevice};
use super::level_meter::{self, LevelMeters};
use super::pw_config;
use super::pw_connection::{Event, PwConnection, Request};
use super::role::Role;
use super::routing::{RoutingRule, StreamInfo, winning_rule_index};
use super::test_tone;
use crate::config::{self, Side};

#[derive(Default)]
pub struct PipeWireBackendImp {
    // Mirrors the pw side state
    sinks: RefCell<HashMap<u32, HwDevice>>,
    sources: RefCell<HashMap<u32, HwDevice>>,
    owned: RefCell<HashMap<Role, u32>>,
    streams: RefCell<HashMap<u32, StreamInfo>>,
    // stream ids the pw thread reports as linked to the Aux sink
    aux_stream_ids: RefCell<HashSet<u32>>,
    // per-stream peaks, written by the capture meters on the pw thread
    stream_peaks: RefCell<HashMap<u32, Arc<AtomicU32>>>,
    // stream ids that we've changed target.object on
    touched: RefCell<HashSet<u32>>,
    default_name: RefCell<Option<String>>,
    default_source_name: RefCell<Option<String>>,

    // gate sinks-ready vs sinks-changed
    installed: Cell<bool>,

    using_temp: Cell<bool>,

    level_meters: RefCell<Option<LevelMeters>>,
    pw: RefCell<Option<PwConnection>>,
}

#[glib::object_subclass]
impl ObjectSubclass for PipeWireBackendImp {
    const NAME: &'static str = "BridgePipeWireBackend";
    type Type = PipeWireBackend;
}

impl ObjectImpl for PipeWireBackendImp {
    fn signals() -> &'static [Signal] {
        static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| {
            vec![
                Signal::builder("sinks-ready").build(),
                Signal::builder("sinks-changed").build(),
                Signal::builder("sources-changed").build(),
                Signal::builder("streams-changed").build(),
                Signal::builder("aux-streams-changed").build(),
                Signal::builder("default-changed").build(),
                Signal::builder("default-source-changed").build(),
                Signal::builder("owned-changed").build(),
                Signal::builder("surround-ready").build(),
                Signal::builder("surround-removed").build(),
                Signal::builder("mic-ready").build(),
                Signal::builder("mic-removed").build(),
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
        let peaks = meters.atoms();
        self.imp().level_meters.replace(Some(meters));

        let (pw, evt_rx) = PwConnection::start(peaks);
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
                self.apply_rules_all();
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
            Event::SourceAdded(source) => {
                imp.sources.borrow_mut().insert(source.node_id, source);
                if imp.installed.get() {
                    self.emit_by_name::<()>("sources-changed", &[]);
                }
            }
            Event::SourceRemoved(id) => {
                let dropped = imp.sources.borrow_mut().remove(&id).is_some();
                if dropped && imp.installed.get() {
                    self.emit_by_name::<()>("sources-changed", &[]);
                }
            }

            Event::NodeReady { role, id } => {
                imp.owned.borrow_mut().insert(role, id);
                match role {
                    Role::Surround => self.emit_by_name::<()>("surround-ready", &[]),
                    Role::Mic => self.emit_by_name::<()>("mic-ready", &[]),
                    _ => self.emit_by_name::<()>("owned-changed", &[]),
                }
            }
            Event::NodeRemoved { role } => {
                let dropped = imp.owned.borrow_mut().remove(&role).is_some();
                match role {
                    Role::Surround => self.emit_by_name::<()>("surround-removed", &[]),
                    Role::Mic => self.emit_by_name::<()>("mic-removed", &[]),
                    _ if dropped => self.emit_by_name::<()>("owned-changed", &[]),
                    _ => {}
                }
            }
            Event::StreamAdded { info, peak } => {
                let live = imp.installed.get();
                if live {
                    self.apply_rule_to_stream(&info, &config::load_rules());
                }
                imp.stream_peaks.borrow_mut().insert(info.node_id, peak);
                imp.streams.borrow_mut().insert(info.node_id, info);
                if live {
                    self.emit_by_name::<()>("streams-changed", &[]);
                }
            }
            Event::StreamRemoved(id) => {
                imp.touched.borrow_mut().remove(&id);
                imp.stream_peaks.borrow_mut().remove(&id);
                let dropped = imp.streams.borrow_mut().remove(&id).is_some();
                if dropped && imp.installed.get() {
                    self.emit_by_name::<()>("streams-changed", &[]);
                }
            }
            Event::AuxStreamsChanged(ids) => {
                imp.aux_stream_ids.replace(ids.into_iter().collect());
                self.emit_by_name::<()>("aux-streams-changed", &[]);
            }
            Event::DefaultSink(raw) => {
                let name = raw.and_then(|v| crate::util::parse_default_name(&v));
                imp.default_name.replace(name);
                self.emit_by_name::<()>("default-changed", &[]);
            }
            Event::DefaultSource(raw) => {
                let name = raw.and_then(|v| crate::util::parse_default_name(&v));
                imp.default_source_name.replace(name);
                self.emit_by_name::<()>("default-source-changed", &[]);
            }
        }
    }

    /// Sorted hardware sinks
    pub fn hw_sinks(&self) -> Vec<HwDevice> {
        let sinks = self.imp().sinks.borrow().values().cloned().collect();
        hw_device::sorted_for_display(sinks)
    }

    /// Sorted hardware sources
    pub fn hw_sources(&self) -> Vec<HwDevice> {
        let sources = self.imp().sources.borrow().values().cloned().collect();
        hw_device::sorted_for_display(sources)
    }

    pub fn output_streams(&self) -> Vec<StreamInfo> {
        self.imp().streams.borrow().values().cloned().collect()
    }

    pub fn aux_streams(&self) -> Vec<StreamInfo> {
        let imp = self.imp();
        let streams = imp.streams.borrow();
        imp.aux_stream_ids
            .borrow()
            .iter()
            .filter_map(|id| streams.get(id).cloned())
            .collect()
    }

    pub fn aux_stream_count(&self) -> usize {
        self.imp().aux_stream_ids.borrow().len()
    }

    pub fn owned_sinks_present(&self) -> bool {
        self.present(Role::Aux) && self.present(Role::Main)
    }

    pub fn present(&self, role: Role) -> bool {
        self.imp().owned.borrow().contains_key(&role)
    }

    /// True while sink is a session-only loopback we loaded, rather than a
    /// persistent one from the conf
    pub fn using_temp_sinks(&self) -> bool {
        self.imp().using_temp.get()
    }

    // The mic is configured separately from the outputs
    fn temp_module_configs(&self) -> Vec<(Role, String)> {
        let owned = self.imp().owned.borrow();
        let mut configs = Vec::new();

        if config::is_configured() {
            let cfg = config::load();
            configs.extend(
                [Side::Aux, Side::Main]
                    .into_iter()
                    .filter(|side| !owned.contains_key(&Role::from(*side)))
                    .map(|side| {
                        (
                            Role::from(side),
                            pw_config::loopback_module_args(side, cfg.side(side)),
                        )
                    }),
            );
        }

        if config::mic_configured() && !owned.contains_key(&Role::Mic) {
            configs.push((Role::Mic, pw_config::mic_module_args(&config::load_mic())));
        }

        configs
    }

    // The persist banner is for the outputs
    fn set_using_temp(&self, configs: &[(Role, String)]) {
        let outputs = configs
            .iter()
            .any(|(role, _)| matches!(role, Role::Aux | Role::Main));
        self.imp().using_temp.set(outputs);
    }

    /// Create in-process loopback for any configured side that isn't
    /// already live with a persistent sink
    pub fn create_missing_temp_sinks(&self) {
        let configs = self.temp_module_configs();
        self.set_using_temp(&configs);
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
        let configs = self.temp_module_configs();
        self.set_using_temp(&configs);
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::RecreateTempSinks(configs));
        }
    }

    /// Live routing of one of our sinks to a hardware output by node.name
    /// None targets the system default; The conf writes the target for new sessions
    pub fn retarget(&self, role: Role, hw_name: &str) {
        let hw_name = (!hw_name.is_empty()).then(|| hw_name.to_owned());
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::Retarget { role, hw_name });
        }
    }

    pub fn apply_rules_all(&self) {
        let rules = config::load_rules();
        for info in self.imp().streams.borrow().values() {
            self.apply_rule_to_stream(info, &rules);
        }
    }

    fn apply_rule_to_stream(&self, info: &StreamInfo, rules: &[RoutingRule]) {
        let imp = self.imp();

        let winner = winning_rule_index(rules, info);
        let target = winner.map(|i| rules[i].target.node_name());

        // Only clear streams we've changed the target.object on
        if target.is_none() && !imp.touched.borrow_mut().remove(&info.node_id) {
            return;
        }
        if target.is_some() {
            imp.touched.borrow_mut().insert(info.node_id);
        }

        if let Some(pw) = imp.pw.borrow().as_ref() {
            pw.send(Request::RetargetStream {
                id: info.node_id,
                target,
            });
        }
    }

    /// Sets the volume on one of our sinks
    pub fn set_volume(&self, role: Role, volume: f64) {
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::SetVolume {
                role,
                volume: volume as f32,
            });
        }
    }

    /// Toggle mutes sink
    pub fn set_mute(&self, role: Role, muted: bool) {
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::SetMute { role, muted });
        }
    }

    /// Play a per-channel test tone through one of our virtual sinks.
    /// Sweeps clockwise starting from FL; LFE last
    pub fn play_test_tone(&self, role: Role, on_done: impl FnOnce() + Send + 'static) {
        let (n_channels, position) = match role {
            Role::Aux => side_layout(Side::Aux),
            Role::Main => side_layout(Side::Main),
            Role::Surround => (
                pw_config::SURROUND_CHANNELS,
                pw_config::SURROUND_POSITION.to_owned(),
            ),
            // no test tone for Mic
            Role::Mic => {
                on_done();
                return;
            }
        };

        let positions = test_tone::pos_str_to_spa_ids(&position, n_channels);
        let sweep = test_tone::clockwise_sweep(&positions);

        let sink_name = pw_config::node_name(role);
        test_tone::play_through_sink(sink_name, n_channels, positions, sweep, on_done);
    }

    /// Get the latest peak level on one of our sinks
    pub fn peak(&self, role: Role) -> f32 {
        self.imp()
            .level_meters
            .borrow()
            .as_ref()
            .map_or(0.0, |m| m.peak(role))
    }

    /// Latest peak on one tracked app stream
    pub fn stream_peak(&self, id: u32) -> f32 {
        self.imp()
            .stream_peaks
            .borrow()
            .get(&id)
            .map_or(0.0, |a| level_meter::take_peak(a))
    }

    pub fn set_default_sink(&self, name: &str) {
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::SetDefault(name.to_owned()));
        }
    }

    pub fn set_main_default(&self) {
        self.set_default_sink(pw_config::MAIN_SINK);
    }

    /// node.name of the current system default sink
    pub fn default_sink_name(&self) -> Option<String> {
        self.imp().default_name.borrow().clone()
    }

    pub fn is_default(&self, name: &str) -> Option<bool> {
        self.default_sink_name().map(|current| current == name)
    }

    pub fn main_is_default(&self) -> Option<bool> {
        self.is_default(pw_config::MAIN_SINK)
    }

    pub fn set_default_source(&self, name: &str) {
        if let Some(pw) = self.imp().pw.borrow().as_ref() {
            pw.send(Request::SetDefaultSource(name.to_owned()));
        }
    }

    pub fn set_mic_default(&self) {
        self.set_default_source(pw_config::MIC_SOURCE);
    }

    /// node.name of the system default source
    pub fn default_source_name(&self) -> Option<String> {
        self.imp().default_source_name.borrow().clone()
    }

    pub fn mic_is_default(&self) -> Option<bool> {
        self.default_source_name()
            .map(|current| current == pw_config::MIC_SOURCE)
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

    pub fn connect_sources_changed<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("sources-changed", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }

    pub fn connect_streams_changed<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("streams-changed", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }

    pub fn connect_aux_streams_changed<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("aux-streams-changed", false, move |args| {
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

    pub fn connect_default_source_changed<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("default-source-changed", false, move |args| {
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

    pub fn connect_surround_ready<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("surround-ready", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }

    pub fn connect_surround_removed<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("surround-removed", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }

    pub fn connect_mic_ready<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("mic-ready", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }

    pub fn connect_mic_removed<F: Fn(&Self) + 'static>(&self, f: F) {
        self.connect_local("mic-removed", false, move |args| {
            let be = args[0].get::<PipeWireBackend>().unwrap();
            f(&be);
            None
        });
    }
}

// Channel count + layout
fn side_layout(side: Side) -> (u32, String) {
    let cfg = config::load();
    let def = cfg.side(side);

    (def.channels.max(2), def.position.clone())
}
