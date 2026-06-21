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

// a capture stream per virtual sink
// runs on the pw_connection thread to pin its target.object in the metadata

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use pipewire as pw;
use pw::stream::StreamState;
use pw::{properties::properties, spa};
use spa::pod::Pod;

use super::State;
use crate::audio::level_meter::peak_f32le;

const SAMPLE_RATE: u32 = 48_000;

pub(super) fn open_meter_stream<'c>(
    core: &'c pw::core::Core,
    sink_name: &str,
    atomic: Arc<AtomicU32>,
    state: &Rc<RefCell<State>>,
) -> Result<
    (
        pw::stream::StreamBox<'c>,
        pw::stream::StreamListener<Arc<AtomicU32>>,
    ),
    pw::Error,
> {
    let stream_name = format!("dashboard-meter-{sink_name}");
    let stream = pw::stream::StreamBox::new(
        core,
        &stream_name,
        properties! {
            *pw::keys::MEDIA_TYPE     => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE     => "Filter",
            *pw::keys::TARGET_OBJECT  => sink_name,
            *pw::keys::NODE_NAME      => stream_name.as_str(),
            *pw::keys::AUDIO_CHANNELS => "1",

            // Important for WP to link to a sink's monitor
            "stream.capture.sink"     => "true",
        },
    )?;

    let listener = stream
        .add_local_listener_with_user_data(atomic)
        .process(|stream, atomic| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }
            let data = &mut datas[0];
            let size = data.chunk().size() as usize;
            let offset = data.chunk().offset() as usize;
            let Some(slice) = data.data() else { return };
            let end = (offset + size).min(slice.len());

            let peak = peak_f32le(&slice[offset..end]);
            if peak > 0.0 {
                atomic.fetch_max(peak.to_bits(), Ordering::Relaxed);
            }
        })
        .state_changed({
            let state = state.clone();
            let sink_name = sink_name.to_owned();
            let pinned = Cell::new(false);
            move |stream, _atomic, _old, new| {
                if new != StreamState::Streaming || pinned.get() {
                    return;
                }

                if let Some((meta, _)) = state.borrow().metadata.as_ref() {
                    meta.set_property(
                        stream.node_id(),
                        "target.object",
                        Some("Spa:String"),
                        Some(&sink_name),
                    );
                    pinned.set(true);
                }
            }
        })
        .register()?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(SAMPLE_RATE);
    audio_info.set_channels(1);

    let mut pos_arr = [0u32; spa::param::audio::MAX_CHANNELS];
    pos_arr[0] = pw::spa::sys::SPA_AUDIO_CHANNEL_MONO;
    audio_info.set_position(pos_arr);

    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: pw::spa::sys::SPA_TYPE_OBJECT_Format,
            id: pw::spa::sys::SPA_PARAM_EnumFormat,
            properties: audio_info.into(),
        }),
    )
    .map_err(|_| pw::Error::CreationFailed)?
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).ok_or(pw::Error::CreationFailed)?];

    stream.connect(
        spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    Ok((stream, listener))
}
