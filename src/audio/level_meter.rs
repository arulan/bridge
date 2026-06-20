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

// Per-sink level meters. One capture pw_stream per virtual sink reads from the
// sink's monitor
//
// Lives in its own thread to avoid issues with the GTK loop

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;

use crate::audio::pw_config::{AUX_SINK, MAIN_SINK};
use crate::config::Side;

const SAMPLE_RATE: u32 = 48_000;

/// One peak atomic per virtual sink.
/// Each holds the loudest sample seen since the last read.
pub struct LevelMeters {
    aux: Arc<AtomicU32>,
    main: Arc<AtomicU32>,
    // Detached
    _thread: std::thread::JoinHandle<()>,
}

impl LevelMeters {
    pub fn start() -> Self {
        let aux = Arc::new(AtomicU32::new(0));
        let main = Arc::new(AtomicU32::new(0));

        let sinks = [(AUX_SINK, Arc::clone(&aux)), (MAIN_SINK, Arc::clone(&main))];

        let thread = std::thread::spawn(move || {
            if let Err(e) = run_thread(sinks) {
                eprintln!("level_meter: thread exited with error: {e}");
            }
        });

        LevelMeters {
            aux,
            main,
            _thread: thread,
        }
    }

    /// Peak observed since the last call
    pub fn peak(&self, side: Side) -> f32 {
        let atomic = match side {
            Side::Aux => &self.aux,
            Side::Main => &self.main,
        };
        f32::from_bits(atomic.swap(0, Ordering::Relaxed))
    }
}

fn run_thread(sinks: [(&'static str, Arc<AtomicU32>); 2]) -> Result<(), pw::Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    // Streams and listeners must outlive mainloop.run()
    let mut streams = Vec::with_capacity(sinks.len());
    let mut listeners = Vec::with_capacity(sinks.len());

    for (sink_name, atomic) in sinks {
        let (stream, listener) = open_meter_stream(&core, sink_name, atomic)?;
        streams.push(stream);
        listeners.push(listener);
    }

    mainloop.run();
    drop(listeners);
    drop(streams);
    Ok(())
}

fn open_meter_stream<'c>(
    core: &'c pw::core::Core,
    sink_name: &str,
    atomic: Arc<AtomicU32>,
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

fn peak_f32le(slice: &[u8]) -> f32 {
    let mut peak = 0.0f32;
    let mut i = 0;
    while i + 4 <= slice.len() {
        let v = f32::from_le_bytes([slice[i], slice[i + 1], slice[i + 2], slice[i + 3]]);
        let a = v.abs();
        if a > peak {
            peak = a;
        }
        i += 4;
    }
    peak
}

#[cfg(test)]
mod tests {
    use super::peak_f32le;

    #[test]
    fn picks_largest_magnitude() {
        let mut buf = Vec::new();
        for s in [0.1f32, -0.7, 0.3, -0.2] {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        assert_eq!(peak_f32le(&buf), 0.7);
    }

    #[test]
    fn skips_trailing_partial_frame() {
        let mut buf = 0.5f32.to_le_bytes().to_vec();
        buf.extend_from_slice(&[0xff, 0xff]); // stray two bytes, not a full frame
        assert_eq!(peak_f32le(&buf), 0.5);
    }

    #[test]
    fn empty_buffer_is_zero() {
        assert_eq!(peak_f32le(&[]), 0.0);
    }
}
