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

// The PW connection on its own thread. One (pw::channel) for outgoing commands
// and the async-channel for events coming back. This replaces the libwireplumber
// WpCore entirely

mod ffi;
mod meter;
mod pod;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::types::ObjectType;

use crate::audio::hw_device::{HwDevice, sink_from_props, source_from_props};
use crate::audio::pw_config;
use crate::audio::role::Role;
use crate::audio::routing::StreamInfo;

use ffi::{LoadedModule, load_module};
use pod::set_node_props;

// Used by stop() waiting for the pw thread to finish flushing
const FLUSH_TIMEOUT: Duration = Duration::from_millis(250);

// main -> pw
pub enum Request {
    SetVolume { role: Role, volume: f32 },
    SetMute { role: Role, muted: bool },
    Retarget { role: Role, hw_name: Option<String> },
    RetargetStream { id: u32, target: Option<String> },
    SetDefault(String),
    SetDefaultSource(String),
    MicMeter { enabled: bool },
    // (role, module args) for each configured role
    // skipped for live sink/module roles
    CreateTempSinks(Vec<(Role, String)>),
    RecreateTempSinks(Vec<(Role, String)>),
    Shutdown,
}

// pw -> main
pub enum Event {
    Settled,
    SinkAdded(HwDevice),
    SinkRemoved(u32),
    SourceAdded(HwDevice),
    SourceRemoved(u32),
    NodeReady {
        role: Role,
        id: u32,
    },
    NodeRemoved {
        role: Role,
    },
    ModuleFailed {
        role: Role,
    },
    StreamAdded {
        info: StreamInfo,
        peak: Arc<AtomicU32>,
    },
    StreamRemoved(u32),
    // The app streams currently linked to the Aux sink
    AuxStreamsChanged(Vec<u32>),
    DefaultSink(Option<String>),
    DefaultSource(Option<String>),
}

pub struct PwConnection {
    cmd_tx: pw::channel::Sender<Request>,
    ack_rx: mpsc::Receiver<()>,
    _join: JoinHandle<()>,
}

impl PwConnection {
    pub fn start(peaks: Vec<(Role, Arc<AtomicU32>)>) -> (Self, async_channel::Receiver<Event>) {
        let (cmd_tx, cmd_rx) = pw::channel::channel::<Request>();
        let (evt_tx, evt_rx) = async_channel::unbounded::<Event>();
        let (ack_tx, ack_rx) = mpsc::channel::<()>();

        let join = std::thread::spawn(move || {
            if let Err(e) = pw_main(cmd_rx, evt_tx, ack_tx, peaks) {
                eprintln!("pw_connection: exited with error: {e}");
            }
        });

        (
            PwConnection {
                cmd_tx,
                ack_rx,
                _join: join,
            },
            evt_rx,
        )
    }

    pub fn send(&self, req: Request) {
        let _ = self.cmd_tx.send(req);
    }

    // Requests the reset and tear down, then flush after timeout
    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(Request::Shutdown);
        let _ = self.ack_rx.recv_timeout(FLUSH_TIMEOUT);
    }
}

// One of our loopback capture nodes
struct OwnedNode {
    id: u32,
    channels: u32,
}

struct State {
    // Every node we've bound
    bound: HashMap<u32, (pw::node::Node, pw::node::NodeListener)>,
    // Our owned capture nodes, the ones we set volume and mute on
    owned: HashMap<Role, OwnedNode>,
    // Owned playback node ids, the targets we change for live routing
    owned_pb: HashMap<Role, u32>,
    // The hardware sink ids we report with SinkAdded
    hw: HashSet<u32>,
    // and the hardware source ids, mics
    hw_sources: HashSet<u32>,
    // streams for routing rules
    streams: HashSet<u32>,
    // every link in the graph
    links: HashMap<u32, (u32, u32)>,
    // last set of streams we reported as linked to Aux
    aux_stream_ids: BTreeSet<u32>,
    // per-stream capture meters, keyed by the stream
    meters: HashMap<u32, meter::StreamMeter>,
    mic_peak: Option<Arc<AtomicU32>>,
    mic_meter: Option<meter::StreamMeter>,
    mic_meter_wanted: bool,

    metadata: Option<(pw::metadata::Metadata, pw::metadata::MetadataListener)>,
    meta_cache: HashMap<String, String>,
    modules: HashMap<Role, LoadedModule>,

    shutting_down: bool,
}

impl State {
    fn new() -> Self {
        State {
            bound: HashMap::new(),
            owned: HashMap::new(),
            owned_pb: HashMap::new(),
            hw: HashSet::new(),
            hw_sources: HashSet::new(),
            streams: HashSet::new(),
            links: HashMap::new(),
            aux_stream_ids: BTreeSet::new(),
            meters: HashMap::new(),
            mic_peak: None,
            mic_meter: None,
            mic_meter_wanted: false,
            metadata: None,
            meta_cache: HashMap::new(),
            modules: HashMap::new(),
            shutting_down: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Registry,
    Props,
    Settled,
}

fn pw_main(
    cmd_rx: pw::channel::Receiver<Request>,
    evt_tx: async_channel::Sender<Event>,
    ack_tx: mpsc::Sender<()>,
    peaks: Vec<(Role, Arc<AtomicU32>)>,
) -> Result<(), pw::Error> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;

    // media.category = Manager is required to get the flatpak client access
    // to the full graph
    let core = context.connect_rc(Some(properties! {
        *pw::keys::MEDIA_CATEGORY => "Manager",
    }))?;

    let registry = core.get_registry_rc()?;
    let state = Rc::new(RefCell::new(State::new()));

    // Requires two syncs; The first 'done' when the registry dump finishes
    // and we have our binds for every node; The second for the full props
    // on the bind's info reply. Only considered settled after the second
    let phase = Cell::new(Phase::Registry);
    let _core_listener = core
        .add_listener_local()
        .done({
            let evt_tx = evt_tx.clone();
            let mainloop = mainloop.clone();
            let state = state.clone();
            let core = core.clone();
            move |id, _seq| {
                if id != pw::core::PW_ID_CORE {
                    return;
                }
                if state.borrow().shutting_down {
                    let _ = ack_tx.send(());
                    mainloop.quit();
                    return;
                }
                match phase.get() {
                    Phase::Registry => {
                        phase.set(Phase::Props);
                        let _ = core.sync(0);
                    }
                    Phase::Props => {
                        phase.set(Phase::Settled);
                        let _ = evt_tx.try_send(Event::Settled);
                    }
                    Phase::Settled => {}
                }
            }
        })
        .register();

    let _registry_listener = registry
        .add_listener_local()
        .global({
            let registry = registry.clone();
            let core = core.clone();
            let evt_tx = evt_tx.clone();
            let state = state.clone();
            move |global| handle_global(global, &registry, &core, &evt_tx, &state)
        })
        .global_remove({
            let evt_tx = evt_tx.clone();
            let state = state.clone();
            move |id| handle_global_remove(id, &evt_tx, &state)
        })
        .register();

    let _recv = cmd_rx.attach(mainloop.loop_(), {
        let core = core.clone();
        let context = context.clone();
        let state = state.clone();
        let evt_tx = evt_tx.clone();
        move |req| handle_request(req, &core, &context, &state, &evt_tx)
    });

    // capture meters run on this thread so they can be pinned in metadata
    // autoconnects when their target sinks appear
    let mut meters = Vec::with_capacity(peaks.len());
    for (role, atomic) in peaks {
        if role == Role::Mic {
            state.borrow_mut().mic_peak = Some(atomic);
            continue;
        }
        let node = pw_config::node_name(role);
        match meter::open_sink_meter(&core, node, atomic, &state) {
            Ok(pair) => meters.push(pair),
            Err(e) => eprintln!("pw_connection: meter stream for {node} failed: {e}"),
        }
    }

    // So the server 'done' fires once the globals are in
    let _ = core.sync(0);

    mainloop.run();
    drop(meters);
    Ok(())
}

fn handle_request(
    req: Request,
    core: &pw::core::CoreRc,
    context: &pw::context::ContextRc,
    state: &Rc<RefCell<State>>,
    evt_tx: &async_channel::Sender<Event>,
) {
    match req {
        Request::SetVolume { role, volume } => {
            let st = state.borrow();
            if let Some(owned) = st.owned.get(&role)
                && let Some((node, _)) = st.bound.get(&owned.id)
            {
                set_node_props(node, Some((volume, owned.channels)), None);
            }
        }
        Request::SetMute { role, muted } => {
            let st = state.borrow();
            if let Some(owned) = st.owned.get(&role)
                && let Some((node, _)) = st.bound.get(&owned.id)
            {
                set_node_props(node, None, Some(muted));
            }
        }
        Request::Retarget { role, hw_name } => {
            let st = state.borrow();
            let (Some(&subject), Some((meta, _))) = (st.owned_pb.get(&role), st.metadata.as_ref())
            else {
                return;
            };
            set_target_object(meta, subject, hw_name.as_deref());
        }
        Request::RetargetStream { id, target } => {
            let st = state.borrow();
            let Some((meta, _)) = st.metadata.as_ref() else {
                return;
            };
            set_target_object(meta, id, target.as_deref());
        }
        Request::SetDefault(name) => {
            let st = state.borrow();
            if let Some((meta, _)) = st.metadata.as_ref() {
                set_configured_default(meta, "default.configured.audio.sink", &name);
            }
        }
        Request::SetDefaultSource(name) => {
            let st = state.borrow();
            if let Some((meta, _)) = st.metadata.as_ref() {
                set_configured_default(meta, "default.configured.audio.source", &name);
            }
        }
        Request::MicMeter { enabled } => {
            {
                let mut st = state.borrow_mut();
                st.mic_meter_wanted = enabled;
                if !enabled {
                    // lets the mic be released
                    st.mic_meter = None;
                }
            }
            let present = state.borrow().owned.contains_key(&Role::Mic);
            if present {
                open_mic_meter(core, state);
            }
        }
        Request::CreateTempSinks(configs) => {
            load_temp_sinks(context, state, evt_tx, configs, &HashSet::new());
        }
        Request::RecreateTempSinks(configs) => {
            let rebuild: HashSet<Role> = state.borrow().modules.keys().copied().collect();
            {
                let mut st = state.borrow_mut();
                if rebuild.contains(&Role::Mic) {
                    st.mic_meter = None;
                }
                st.modules.clear();
            }
            load_temp_sinks(context, state, evt_tx, configs, &rebuild);
        }
        Request::Shutdown => {
            // Return our owned sinks to 1.0 volume and unmuted,
            // then sync so the writes flush before the 'done' handler quits
            {
                let st = state.borrow();
                for owned in st.owned.values() {
                    if let Some((node, _)) = st.bound.get(&owned.id) {
                        set_node_props(node, Some((1.0, owned.channels)), Some(false));
                    }
                }
            }
            state.borrow_mut().shutting_down = true;
            let _ = core.sync(0);
        }
    }
}

fn load_temp_sinks(
    context: &pw::context::ContextRc,
    state: &Rc<RefCell<State>>,
    evt_tx: &async_channel::Sender<Event>,
    configs: Vec<(Role, String)>,
    rebuild: &HashSet<Role>,
) {
    for (role, args) in configs {
        {
            let st = state.borrow();
            if skip_load(&st, role, rebuild) {
                continue;
            }
        }
        match load_module(context, "libpipewire-module-loopback", &args) {
            Some(m) => {
                state.borrow_mut().modules.insert(role, m);
            }
            None => {
                eprintln!("pw_connection: failed to load temp loopback for {role:?}");
                let _ = evt_tx.try_send(Event::ModuleFailed { role });
            }
        }
    }
}

fn skip_load(st: &State, role: Role, rebuild: &HashSet<Role>) -> bool {
    let live = st.owned.contains_key(&role);
    let loaded = st.modules.contains_key(&role);
    loaded || (live && !rebuild.contains(&role))
}

fn handle_global(
    global: &pw::registry::GlobalObject<&spa::utils::dict::DictRef>,
    registry: &pw::registry::RegistryRc,
    core: &pw::core::CoreRc,
    evt_tx: &async_channel::Sender<Event>,
    state: &Rc<RefCell<State>>,
) {
    match global.type_ {
        ObjectType::Metadata => {
            let Some(props) = global.props else { return };
            if props.get("metadata.name") != Some("default") {
                return;
            }
            if state.borrow().metadata.is_some() {
                return;
            }
            let Ok(meta) = registry.bind::<pw::metadata::Metadata, _>(global) else {
                return;
            };
            let listener = meta
                .add_listener_local()
                .property({
                    let evt_tx = evt_tx.clone();
                    let state = state.clone();
                    move |_subject, key, _type, value| {
                        if let Some(key) = key {
                            match value {
                                Some(v) => {
                                    state
                                        .borrow_mut()
                                        .meta_cache
                                        .insert(key.to_owned(), v.to_owned());
                                }
                                None => {
                                    state.borrow_mut().meta_cache.remove(key);
                                }
                            }
                            match key {
                                "default.audio.sink" => {
                                    let _ = evt_tx
                                        .try_send(Event::DefaultSink(value.map(str::to_owned)));
                                }
                                "default.audio.source" => {
                                    let _ = evt_tx
                                        .try_send(Event::DefaultSource(value.map(str::to_owned)));
                                }
                                _ => {}
                            }
                        }
                        0
                    }
                })
                .register();
            state.borrow_mut().metadata = Some((meta, listener));
        }

        ObjectType::Node => {
            let Some(props) = global.props else { return };

            // Filter out the nodes we don't care about
            let ours = props
                .get("node.name")
                .is_some_and(|n| n.starts_with("bridge_"));
            let class = props.get("media.class");
            let wanted = matches!(
                class,
                Some("Audio/Sink") | Some("Audio/Source") | Some("Stream/Output/Audio")
            );
            if !wanted && !ours {
                return;
            }

            let id = global.id;
            if state.borrow().bound.contains_key(&id) {
                return;
            }
            let Ok(node) = registry.bind::<pw::node::Node, _>(global) else {
                return;
            };

            // We get the full prop set on the node's info event
            let classified = Cell::new(false);
            let listener = node
                .add_listener_local()
                .info({
                    let core = core.clone();
                    let evt_tx = evt_tx.clone();
                    let state = state.clone();
                    move |info| {
                        if classified.replace(true) {
                            return;
                        }
                        let Some(props) = info.props() else { return };
                        classify_node(info.id(), props, &core, &evt_tx, &state);
                    }
                })
                .register();

            state.borrow_mut().bound.insert(id, (node, listener));
        }

        ObjectType::Link => {
            let Some(props) = global.props else { return };
            let out = props.get("link.output.node").and_then(|s| s.parse().ok());
            let inp = props.get("link.input.node").and_then(|s| s.parse().ok());
            let (Some(out), Some(inp)) = (out, inp) else {
                return;
            };

            let touches_aux = aux_sink_id(state) == Some(inp);
            state.borrow_mut().links.insert(global.id, (out, inp));
            if touches_aux {
                refresh_aux_streams(evt_tx, state);
            }
        }

        _ => {}
    }
}

fn aux_sink_id(state: &Rc<RefCell<State>>) -> Option<u32> {
    state.borrow().owned.get(&Role::Aux).map(|o| o.id)
}

fn refresh_aux_streams(evt_tx: &async_channel::Sender<Event>, state: &Rc<RefCell<State>>) {
    let mut st = state.borrow_mut();
    let ids: BTreeSet<u32> = match st.owned.get(&Role::Aux).map(|o| o.id) {
        Some(aux_id) => {
            let outs: Vec<u32> = st
                .links
                .values()
                .filter(|(_, inp)| *inp == aux_id)
                .map(|(out, _)| *out)
                .collect();
            outs.into_iter()
                .filter(|out| st.streams.contains(out))
                .collect()
        }
        None => BTreeSet::new(),
    };

    if ids == st.aux_stream_ids {
        return;
    }
    st.aux_stream_ids = ids.clone();
    let _ = evt_tx.try_send(Event::AuxStreamsChanged(ids.into_iter().collect()));
}

fn classify_node(
    id: u32,
    props: &spa::utils::dict::DictRef,
    core: &pw::core::CoreRc,
    evt_tx: &async_channel::Sender<Event>,
    state: &Rc<RefCell<State>>,
) {
    if let Some(role) = props.get("bridge.role").and_then(Role::from_wire) {
        // only a fallback; confs write audio.channels
        let default_channels = match role {
            Role::Surround => pw_config::SURROUND_CHANNELS,
            Role::Mic => 1,
            Role::Aux | Role::Main => 2,
        };
        let channels = props
            .get("audio.channels")
            .and_then(|s| s.parse().ok())
            .unwrap_or(default_channels);
        state
            .borrow_mut()
            .owned
            .insert(role, OwnedNode { id, channels });

        if role == Role::Mic {
            open_mic_meter(core, state);
        }

        let _ = evt_tx.try_send(Event::NodeReady { role, id });
        refresh_aux_streams(evt_tx, state);
        return;
    }

    if let Some(role) = props.get("bridge.pb-role").and_then(Role::from_wire) {
        state.borrow_mut().owned_pb.insert(role, id);
        return;
    }

    if let Some(info) = stream_info_from_props(id, props) {
        let peak = Arc::new(AtomicU32::new(0));
        match dict_prop(props, "object.serial") {
            Some(serial) => match meter::open_stream_meter(core, id, &serial, Arc::clone(&peak)) {
                Ok(m) => {
                    state.borrow_mut().meters.insert(id, m);
                }
                Err(e) => eprintln!("pw_connection: meter for stream {id} failed: {e}"),
            },
            None => eprintln!("pw_connection: stream {id} has no object.serial, not metering"),
        }
        state.borrow_mut().streams.insert(id);
        let _ = evt_tx.try_send(Event::StreamAdded { info, peak });
        refresh_aux_streams(evt_tx, state);
        return;
    }

    if let Some(sink) = sink_from_props(id, props) {
        state.borrow_mut().hw.insert(id);
        let _ = evt_tx.try_send(Event::SinkAdded(sink));
        return;
    }

    if let Some(source) = source_from_props(id, props) {
        state.borrow_mut().hw_sources.insert(id);
        let _ = evt_tx.try_send(Event::SourceAdded(source));
    }
}

fn handle_global_remove(
    id: u32,
    evt_tx: &async_channel::Sender<Event>,
    state: &Rc<RefCell<State>>,
) {
    let mut refresh = false;
    {
        let mut st = state.borrow_mut();

        if let Some(role) = role_for_owned(&st.owned, id) {
            st.owned.remove(&role);
            // with target gone, nothing to capture
            if role == Role::Mic {
                st.mic_meter = None;
            }
            let _ = evt_tx.try_send(Event::NodeRemoved { role });
            refresh = role == Role::Aux;
        } else if let Some(role) = role_for_pb(&st.owned_pb, id) {
            st.owned_pb.remove(&role);
        } else if st.hw.remove(&id) {
            let _ = evt_tx.try_send(Event::SinkRemoved(id));
        } else if st.hw_sources.remove(&id) {
            let _ = evt_tx.try_send(Event::SourceRemoved(id));
        } else if st.streams.remove(&id) {
            st.meters.remove(&id);
            let _ = evt_tx.try_send(Event::StreamRemoved(id));
            refresh = true;
        } else if let Some((_, inp)) = st.links.remove(&id) {
            refresh = st.owned.get(&Role::Aux).map(|o| o.id) == Some(inp);
        }

        st.bound.remove(&id);
    }

    if refresh {
        refresh_aux_streams(evt_tx, state);
    }
}

fn stream_info_from_props(id: u32, props: &spa::utils::dict::DictRef) -> Option<StreamInfo> {
    if props.get("media.class") != Some("Stream/Output/Audio") {
        return None;
    }
    if props.get("bridge.role").is_some() || props.get("bridge.pb-role").is_some() {
        return None;
    }

    let app_name = dict_prop(props, "application.name");
    let binary = dict_prop(props, "application.process.binary");
    if app_name.is_none() && binary.is_none() {
        return None;
    }

    Some(StreamInfo {
        node_id: id,
        app_name,
        app_icon: dict_prop(props, "application.icon-name"),
        binary,
        media_name: dict_prop(props, "media.name"),
    })
}

fn open_mic_meter(core: &pw::core::CoreRc, state: &Rc<RefCell<State>>) {
    let atomic = {
        let st = state.borrow();

        if !st.mic_meter_wanted || st.mic_meter.is_some() {
            return;
        }
        let Some(atomic) = st.mic_peak.clone() else {
            return;
        };
        atomic
    };
    match meter::open_mic_meter(core, pw_config::MIC_SOURCE, atomic) {
        Ok(m) => {
            state.borrow_mut().mic_meter = Some(m);
        }
        Err(e) => eprintln!("pw_connection: meter stream for the mic failed: {e}"),
    }
}

fn set_configured_default(meta: &pw::metadata::Metadata, key: &str, name: &str) {
    let value = format!("{{\"name\":\"{name}\"}}");
    meta.set_property(0, key, Some("Spa:String:JSON"), Some(&value));
}

fn set_target_object(meta: &pw::metadata::Metadata, subject: u32, target: Option<&str>) {
    match target {
        Some(name) => meta.set_property(subject, "target.object", Some("Spa:String"), Some(name)),
        None => meta.set_property(subject, "target.object", None, None),
    }
}

fn dict_prop(props: &spa::utils::dict::DictRef, key: &str) -> Option<String> {
    props.get(key).map(str::to_owned)
}

fn role_for_owned(owned: &HashMap<Role, OwnedNode>, id: u32) -> Option<Role> {
    owned.iter().find(|&(_, o)| o.id == id).map(|(&r, _)| r)
}

fn role_for_pb(owned_pb: &HashMap<Role, u32>, id: u32) -> Option<Role> {
    owned_pb
        .iter()
        .find(|&(_, &pb_id)| pb_id == id)
        .map(|(&r, _)| r)
}
