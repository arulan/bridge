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

// SPIKE to confirm whether we can move off of libwireplumber and just rely on
// raw pipewire-rs. The pw loop runs on itw own thread and we communicate  with it
// over two channels (pw::channel and async-channel).
//
// cargo run --bin pw_spike
//
// Prints diagnostic info to the console to confirm whether the mechanisms we need
// will work

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Cursor;
use std::rc::Rc;

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Property, PropertyFlags, Value, ValueArray};

const AUX_SINK:  &str = "dashboard_aux";
const MAIN_SINK: &str = "dashboard_main";

// main -> pw
enum Request {
    SetVolume { name: String, channels: u32, volume: f32 },
    GetDefaultSink,
    Shutdown,
}

// pw -> main
enum Event {
    Settled,
    NodeFound { id: u32, name: String },
    NodeInfo {
        id:       u32,
        name:     String,
        channels: u32,
        role:     Option<String>,
        position: Option<String>,
    },
    VolumeSet { name: String },
    DefaultSink(Option<String>),
}

struct NodeEntry {
    id:        u32,
    proxy:     pw::node::Node,
    _listener: pw::node::NodeListener,
}

#[derive(Default)]
struct State {
    nodes:      HashMap<String, NodeEntry>,
    meta_cache: HashMap<String, String>,
    metadata:   Option<(pw::metadata::Metadata, pw::metadata::MetadataListener)>,
}

fn main() {
    let (cmd_tx, cmd_rx) = pw::channel::channel::<Request>();
    let (evt_tx, evt_rx) = async_channel::unbounded::<Event>();

    let pw_thread = std::thread::spawn(move || {
        if let Err(e) = pipewire_thread(cmd_rx, evt_tx) {
            eprintln!("[SPIKE] pw thread error: {e}");
        }
    });

    let main_loop = glib::MainLoop::new(None, false);

    glib::spawn_future_local({
        let cmd_tx = cmd_tx.clone();
        async move {
            while let Ok(evt) = evt_rx.recv().await {
                match evt {
                    Event::Settled => {
                        eprintln!("[SPIKE] registry settled, connected as Manager");
                        let _ = cmd_tx.send(Request::GetDefaultSink);
                    }

                    Event::NodeFound { id, name } => {
                        eprintln!("[SPIKE] found node {id} = {name}");
                    }

                    Event::NodeInfo { id, name, channels, role, position } => {
                        eprintln!(
                            "[SPIKE] node {id} {name}: channels={channels} role={role:?} position={position:?}"
                        );
                        if name == MAIN_SINK {
                            let _ = cmd_tx.send(Request::SetVolume { name, channels, volume: 0.5 });
                        }
                    }

                    Event::VolumeSet { name } => {
                        eprintln!("[SPIKE] volume applied on {name} (round trip ok)");
                    }

                    Event::DefaultSink(v) => {
                        eprintln!("[SPIKE] default.audio.sink = {v:?}");
                    }
                }
            }
        }
    });

    // Walk through it, then bring the pw thread down
    glib::timeout_add_seconds_local_once(5, {
        let main_loop = main_loop.clone();
        move || {
            let _ = cmd_tx.send(Request::Shutdown);
            main_loop.quit();
        }
    });

    main_loop.run();
    let _ = pw_thread.join();
}

fn pipewire_thread(
    cmd_rx: pw::channel::Receiver<Request>,
    evt_tx: async_channel::Sender<Event>,
) -> Result<(), pw::Error> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context  = pw::context::ContextRc::new(&mainloop, None)?;

    // The manager category is what the sandboxed client needs
    let core = context.connect_rc(Some(properties! {
        *pw::keys::MEDIA_CATEGORY => "Manager",
    }))?;

    let registry = core.get_registry_rc()?;
    let state: Rc<RefCell<State>> = Rc::new(RefCell::new(State::default()));

    // A core 'done' in reply to our sync tells us the
    // initial registry dump completes
    let settled = Rc::new(Cell::new(false));
    let _core_listener = core
        .add_listener_local()
        .done({
            let evt_tx  = evt_tx.clone();
            let settled = settled.clone();
            move |id, _seq| {
                if id == pw::core::PW_ID_CORE && !settled.replace(true) {
                    let _ = evt_tx.try_send(Event::Settled);
                }
            }
        })
        .register();

    let _registry_listener = registry
        .add_listener_local()
        .global({
            let registry = registry.clone();
            let evt_tx   = evt_tx.clone();
            let state    = state.clone();
            move |global| handle_global(global, &registry, &evt_tx, &state)
        })
        .global_remove({
            let state = state.clone();
            move |id| {
                state.borrow_mut().nodes.retain(|_, e| e.id != id);
            }
        })
        .register();

    // Commands in from the main thread
    let _recv = cmd_rx.attach(mainloop.loop_(), {
        let evt_tx   = evt_tx.clone();
        let state    = state.clone();
        let mainloop = mainloop.clone();
        move |req| match req {
            Request::SetVolume { name, channels, volume } => {
                let st = state.borrow();
                if let Some(entry) = st.nodes.get(&name) {
                    let bytes = props_pod(Some(volume), channels, None);
                    if let Some(pod) = Pod::from_bytes(&bytes) {
                        entry.proxy.set_param(spa::param::ParamType::Props, 0, pod);
                        let _ = evt_tx.try_send(Event::VolumeSet { name });
                    }
                }
            }
            Request::GetDefaultSink => {
                let v = state.borrow().meta_cache.get("default.audio.sink").cloned();
                let _ = evt_tx.try_send(Event::DefaultSink(v));
            }
            Request::Shutdown => mainloop.quit(),
        }
    });

    // so the 'done' above fires once the current globals are in
    let _ = core.sync(0);

    mainloop.run();
    Ok(())
}

fn handle_global(
    global:   &pw::registry::GlobalObject<&spa::utils::dict::DictRef>,
    registry: &pw::registry::RegistryRc,
    evt_tx:   &async_channel::Sender<Event>,
    state:    &Rc<RefCell<State>>,
) {
    let Some(props) = global.props else { return };

    // The default metadata
    if props.get("metadata.name") == Some("default") {
        if state.borrow().metadata.is_some() {
            return;
        }
        let Ok(meta) = registry.bind::<pw::metadata::Metadata, _>(global) else { return };
        let listener = meta
            .add_listener_local()
            .property({
                let evt_tx = evt_tx.clone();
                let state  = state.clone();
                move |_subject, key, _type, value| {
                    if let Some(key) = key {
                        match value {
                            Some(v) => { state.borrow_mut().meta_cache.insert(key.to_owned(), v.to_owned()); }
                            None    => { state.borrow_mut().meta_cache.remove(key); }
                        }

                        if key == "default.audio.sink" {
                            let _ = evt_tx.try_send(Event::DefaultSink(value.map(str::to_owned)));
                        }
                    }
                    0
                }
            })
            .register();
        state.borrow_mut().metadata = Some((meta, listener));
        return;
    }

    // our virtual sinks
    let Some(name) = props.get("node.name") else { return };
    if name != AUX_SINK && name != MAIN_SINK {
        return;
    }

    if state.borrow().nodes.contains_key(name) {
        return;
    }

    let id = global.id;
    let Ok(node) = registry.bind::<pw::node::Node, _>(global) else {
        eprintln!("[SPIKE] failed to bind node {id} {name}");
        return;
    };

    let sent = Cell::new(false);
    let listener = node
        .add_listener_local()
        .info({
            let evt_tx = evt_tx.clone();
            let name   = name.to_owned();
            move |info| {
                
                if sent.replace(true) {
                    return;
                }

                let dict = info.props();
                let get = |k: &str| dict.and_then(|d| d.get(k)).map(str::to_owned);
                let channels = get("audio.channels")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(2);
                let _ = evt_tx.try_send(Event::NodeInfo {
                    id:       info.id(),
                    name:     name.clone(),
                    channels,
                    role:     get("dashboard.role"),
                    position: get("audio.position"),
                });
            }
        })
        .register();

    state.borrow_mut().nodes.insert(
        name.to_owned(),
        NodeEntry { id, proxy: node, _listener: listener },
    );
    let _ = evt_tx.try_send(Event::NodeFound { id, name: name.to_owned() });
}

// Build up the props, similar to WP's build_props_pod
fn props_pod(volume: Option<f32>, channels: u32, mute: Option<bool>) -> Vec<u8> {
    let mut properties = Vec::new();

    if let Some(v) = volume {
        properties.push(Property {
            key:   spa::sys::SPA_PROP_channelVolumes,
            flags: PropertyFlags::empty(),
            value: Value::ValueArray(ValueArray::Float(vec![v; channels.max(1) as usize])),
        });
    }

    if let Some(m) = mute {
        properties.push(Property {
            key:   spa::sys::SPA_PROP_mute,
            flags: PropertyFlags::empty(),
            value: Value::Bool(m),
        });
    }

    let object = Value::Object(Object {
        type_: spa::sys::SPA_TYPE_OBJECT_Props,
        id:    spa::sys::SPA_PARAM_Props,
        properties,
    });

    PodSerializer::serialize(Cursor::new(Vec::new()), &object)
        .expect("failed to serialize Props pod")
        .0
        .into_inner()
}
