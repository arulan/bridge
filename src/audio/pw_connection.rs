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

use crate::audio::hw_sink::{HwSink, hw_sink_from_props};
use crate::audio::pw_config::{AUX_SINK, MAIN_SINK, SURROUND_SINK};
use crate::audio::routing::StreamInfo;
use crate::config::Side;

use ffi::{LoadedModule, load_module};
use pod::set_node_props;

// Used by stop() waiting for the pw thread to finish flushing
const FLUSH_TIMEOUT: Duration = Duration::from_millis(250);

// main -> pw
pub enum Request {
    SetVolume { side: Side, volume: f32 },
    SetMute { side: Side, muted: bool },
    SetSurroundVolume { volume: f32 },
    SetSurroundMute { muted: bool },
    Retarget { side: Side, hw_name: Option<String> },
    RetargetStream { id: u32, target: Option<String> },
    SetDefault(String),
    // (side, module args) for each configured side
    // skipped for live sink/module sides
    CreateTempSinks(Vec<(Side, String)>),
    RecreateTempSinks(Vec<(Side, String)>),
    Shutdown,
}

// pw -> main
pub enum Event {
    Settled,
    SinkAdded(HwSink),
    SinkRemoved(u32),
    OwnedAdded {
        side: Side,
        id: u32,
    },
    OwnedRemoved {
        side: Side,
    },
    SurroundReady {
        id: u32,
    },
    SurroundRemoved,
    StreamAdded {
        info: StreamInfo,
        peak: Arc<AtomicU32>,
    },
    StreamRemoved(u32),
    // The app streams currently linked to the Aux sink
    AuxStreamsChanged(Vec<u32>),
    DefaultSink(Option<String>),
}

pub struct PwConnection {
    cmd_tx: pw::channel::Sender<Request>,
    ack_rx: mpsc::Receiver<()>,
    _join: JoinHandle<()>,
}

impl PwConnection {
    pub fn start(
        aux_peak: Arc<AtomicU32>,
        main_peak: Arc<AtomicU32>,
        surround_peak: Arc<AtomicU32>,
    ) -> (Self, async_channel::Receiver<Event>) {
        let (cmd_tx, cmd_rx) = pw::channel::channel::<Request>();
        let (evt_tx, evt_rx) = async_channel::unbounded::<Event>();
        let (ack_tx, ack_rx) = mpsc::channel::<()>();

        let join = std::thread::spawn(move || {
            if let Err(e) = pw_main(cmd_rx, evt_tx, ack_tx, aux_peak, main_peak, surround_peak) {
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
struct OwnedSink {
    id: u32,
    channels: u32,
}

struct State {
    // Every node we've bound
    bound: HashMap<u32, (pw::node::Node, pw::node::NodeListener)>,
    // Our owned capture nodes, the ones we set volume and mute on
    owned: HashMap<Side, OwnedSink>,
    // The surround sink, when the node is live
    surround: Option<OwnedSink>,
    // Owned playback node ids, the targets we change for live routing
    owned_pb: HashMap<Side, u32>,
    // The hardware sink ids we report with SinkAdded
    hw: HashSet<u32>,
    // streams for routing rules
    streams: HashSet<u32>,
    // every link in the graph
    links: HashMap<u32, (u32, u32)>,
    // last set of streams we reported as linked to Aux
    aux_stream_ids: BTreeSet<u32>,
    // per-stream capture meters, keyed by the stream
    meters: HashMap<u32, meter::StreamMeter>,

    metadata: Option<(pw::metadata::Metadata, pw::metadata::MetadataListener)>,
    meta_cache: HashMap<String, String>,
    modules: HashMap<Side, LoadedModule>,

    shutting_down: bool,
}

impl State {
    fn new() -> Self {
        State {
            bound: HashMap::new(),
            owned: HashMap::new(),
            surround: None,
            owned_pb: HashMap::new(),
            hw: HashSet::new(),
            streams: HashSet::new(),
            links: HashMap::new(),
            aux_stream_ids: BTreeSet::new(),
            meters: HashMap::new(),
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
    aux_peak: Arc<AtomicU32>,
    main_peak: Arc<AtomicU32>,
    surround_peak: Arc<AtomicU32>,
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
        move |req| handle_request(req, &core, &context, &state)
    });

    // capture meters run on this thread so they can be pinned in metadata
    // autoconnects when their target sinks appear
    let mut meters = Vec::with_capacity(3);
    for (sink, atomic) in [
        (AUX_SINK, aux_peak),
        (MAIN_SINK, main_peak),
        (SURROUND_SINK, surround_peak),
    ] {
        match meter::open_sink_meter(&core, sink, atomic, &state) {
            Ok(pair) => meters.push(pair),
            Err(e) => eprintln!("pw_connection: meter stream for {sink} failed: {e}"),
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
) {
    match req {
        Request::SetVolume { side, volume } => {
            let st = state.borrow();
            if let Some(owned) = st.owned.get(&side)
                && let Some((node, _)) = st.bound.get(&owned.id)
            {
                set_node_props(node, Some((volume, owned.channels)), None);
            }
        }
        Request::SetMute { side, muted } => {
            let st = state.borrow();
            if let Some(owned) = st.owned.get(&side)
                && let Some((node, _)) = st.bound.get(&owned.id)
            {
                set_node_props(node, None, Some(muted));
            }
        }
        Request::SetSurroundVolume { volume } => {
            let st = state.borrow();
            if let Some(s) = st.surround.as_ref()
                && let Some((node, _)) = st.bound.get(&s.id)
            {
                set_node_props(node, Some((volume, s.channels)), None);
            }
        }
        Request::SetSurroundMute { muted } => {
            let st = state.borrow();
            if let Some(s) = st.surround.as_ref()
                && let Some((node, _)) = st.bound.get(&s.id)
            {
                set_node_props(node, None, Some(muted));
            }
        }
        Request::Retarget { side, hw_name } => {
            let st = state.borrow();
            let (Some(&subject), Some((meta, _))) = (st.owned_pb.get(&side), st.metadata.as_ref())
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
                let value = format!("{{\"name\":\"{name}\"}}");
                meta.set_property(
                    0,
                    "default.configured.audio.sink",
                    Some("Spa:String:JSON"),
                    Some(&value),
                );
            }
        }
        Request::CreateTempSinks(configs) => {
            load_temp_sinks(context, state, configs);
        }
        Request::RecreateTempSinks(configs) => {
            state.borrow_mut().modules.clear();
            load_temp_sinks(context, state, configs);
        }
        Request::Shutdown => {
            // Return our owned sinks to 1.0 volume and unmuted,
            // then sync so the writes flush before the 'done' handler quits
            {
                let st = state.borrow();
                for owned in st.owned.values().chain(st.surround.as_ref()) {
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
    configs: Vec<(Side, String)>,
) {
    for (side, args) in configs {
        {
            let st = state.borrow();
            if st.owned.contains_key(&side) || st.modules.contains_key(&side) {
                continue;
            }
        }
        match load_module(context, "libpipewire-module-loopback", &args) {
            Some(m) => {
                state.borrow_mut().modules.insert(side, m);
            }
            None => eprintln!("pw_connection: failed to load temp loopback for {side:?}"),
        }
    }
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
                            if key == "default.audio.sink" {
                                let _ =
                                    evt_tx.try_send(Event::DefaultSink(value.map(str::to_owned)));
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
            if class != Some("Audio/Sink") && class != Some("Stream/Output/Audio") && !ours {
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
    state.borrow().owned.get(&Side::Aux).map(|o| o.id)
}

fn refresh_aux_streams(evt_tx: &async_channel::Sender<Event>, state: &Rc<RefCell<State>>) {
    let mut st = state.borrow_mut();
    let ids: BTreeSet<u32> = match st.owned.get(&Side::Aux).map(|o| o.id) {
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
    let role = props.get("bridge.role");

    if role == Some("surround") {
        let channels = props
            .get("audio.channels")
            .and_then(|s| s.parse().ok())
            .unwrap_or(8);
        state.borrow_mut().surround = Some(OwnedSink { id, channels });
        let _ = evt_tx.try_send(Event::SurroundReady { id });
        return;
    }

    if let Some(side) = role.and_then(Side::from_wire) {
        let channels = props
            .get("audio.channels")
            .and_then(|s| s.parse().ok())
            .unwrap_or(2);
        state
            .borrow_mut()
            .owned
            .insert(side, OwnedSink { id, channels });
        let _ = evt_tx.try_send(Event::OwnedAdded { side, id });
        refresh_aux_streams(evt_tx, state);
        return;
    }

    if let Some(side) = props.get("bridge.pb-role").and_then(Side::from_wire) {
        state.borrow_mut().owned_pb.insert(side, id);
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

    if let Some(sink) = hw_sink_from_props(id, props) {
        state.borrow_mut().hw.insert(id);
        let _ = evt_tx.try_send(Event::SinkAdded(sink));
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

        if st.surround.as_ref().is_some_and(|s| s.id == id) {
            st.surround = None;
            let _ = evt_tx.try_send(Event::SurroundRemoved);
        } else if let Some(side) = side_for_owned(&st.owned, id) {
            st.owned.remove(&side);
            let _ = evt_tx.try_send(Event::OwnedRemoved { side });
            refresh = side == Side::Aux;
        } else if let Some(side) = side_for_pb(&st.owned_pb, id) {
            st.owned_pb.remove(&side);
        } else if st.hw.remove(&id) {
            let _ = evt_tx.try_send(Event::SinkRemoved(id));
        } else if st.streams.remove(&id) {
            st.meters.remove(&id);
            let _ = evt_tx.try_send(Event::StreamRemoved(id));
            refresh = true;
        } else if let Some((_, inp)) = st.links.remove(&id) {
            refresh = st.owned.get(&Side::Aux).map(|o| o.id) == Some(inp);
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

fn set_target_object(meta: &pw::metadata::Metadata, subject: u32, target: Option<&str>) {
    match target {
        Some(name) => meta.set_property(subject, "target.object", Some("Spa:String"), Some(name)),
        None => meta.set_property(subject, "target.object", None, None),
    }
}

fn dict_prop(props: &spa::utils::dict::DictRef, key: &str) -> Option<String> {
    props.get(key).map(str::to_owned)
}

fn side_for_owned(owned: &HashMap<Side, OwnedSink>, id: u32) -> Option<Side> {
    owned.iter().find(|&(_, o)| o.id == id).map(|(&s, _)| s)
}

fn side_for_pb(owned_pb: &HashMap<Side, u32>, id: u32) -> Option<Side> {
    owned_pb
        .iter()
        .find(|&(_, &pb_id)| pb_id == id)
        .map(|(&s, _)| s)
}
