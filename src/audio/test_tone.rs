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

// A per-side test tone; Useful to verify routing and crossfade mixing levels
// Opens a pw_stream and plays a short sine wave on each channel
// Tears itself down once the audio sweep is over

use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;

const SAMPLE_RATE: u32 = 48_000;
const FREQ_HZ: f64 = 440.0;
const LFE_FREQ_HZ: f64 = 60.0;
const AMPLITUDE: f64 = 0.2;
const TONE_DURATION_S: f64 = 0.5;
const PAUSE_DURATION_S: f64 = 0.2;
const WATCHDOG_MARGIN_S: f64 = 2.0; // slack over the sweep duration; fallback
const TWO_PI: f64 = std::f64::consts::PI * 2.0;
const SAMPLE_SIZE: usize = std::mem::size_of::<i16>();

/// Maps channel position to SPA channel Ids
pub fn pos_str_to_spa_ids(s: &str, n: u32) -> Vec<u32> {
    use pw::spa::sys::*;
    s.split(',')
        .take(n as usize)
        .map(|tok| match tok.trim() {
            "FL" => SPA_AUDIO_CHANNEL_FL,
            "FR" => SPA_AUDIO_CHANNEL_FR,
            "FC" => SPA_AUDIO_CHANNEL_FC,
            "LFE" => SPA_AUDIO_CHANNEL_LFE,
            "RL" => SPA_AUDIO_CHANNEL_RL,
            "RR" => SPA_AUDIO_CHANNEL_RR,
            "SL" => SPA_AUDIO_CHANNEL_SL,
            "SR" => SPA_AUDIO_CHANNEL_SR,
            _ => SPA_AUDIO_CHANNEL_UNKNOWN,
        })
        .collect()
}

pub fn play_through_sink(
    sink_name: &str,
    n_channels: u32,
    positions: Vec<u32>,
    sweep_order: Vec<usize>,
    on_done: impl FnOnce() + Send + 'static,
) {
    let target = sink_name.to_owned();
    std::thread::spawn(move || {
        if let Err(e) = run(&target, n_channels, positions, sweep_order) {
            eprintln!("test_tone: pw_stream playback failed: {e}");
        }
        glib::idle_add_once(on_done);
    });
}

fn run(
    sink_name: &str,
    n_channels: u32,
    positions: Vec<u32>,
    sweep_order: Vec<usize>,
) -> Result<(), pw::Error> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let ch_str = n_channels.to_string();
    let stream = pw::stream::StreamBox::new(
        &core,
        "dashboard-test-tone",
        properties! {
            *pw::keys::MEDIA_TYPE     => "Audio",
            *pw::keys::MEDIA_ROLE     => "Test",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::TARGET_OBJECT  => sink_name,
            *pw::keys::NODE_NAME      => "dashboard-test-tone",
            *pw::keys::AUDIO_CHANNELS => ch_str.as_str(),
        },
    )?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::S16LE);
    audio_info.set_rate(SAMPLE_RATE);
    audio_info.set_channels(n_channels);
    let mut pos_arr = [0u32; spa::param::audio::MAX_CHANNELS];
    for (i, &p) in positions.iter().enumerate() {
        pos_arr[i] = p;
    }
    audio_info.set_position(pos_arr);

    let tone_samples = (SAMPLE_RATE as f64 * TONE_DURATION_S) as u32;
    let pause_samples = (SAMPLE_RATE as f64 * PAUSE_DURATION_S) as u32;

    // pattern is tone -> pause for each channel
    let total_steps = sweep_order.len() * 2;
    let sweep_secs = sweep_order.len() as f64 * (TONE_DURATION_S + PAUSE_DURATION_S);

    let mainloop_quit = mainloop.clone();
    let state_quit = mainloop.clone();

    // State: (step index, samples emitted in step, phase accumulator)
    let state = (0usize, 0u32, 0.0f64);

    let lfe_id = pw::spa::sys::SPA_AUDIO_CHANNEL_LFE;

    let _listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(move |_stream, _ud, _old, new| {
            // WP refused or dropped the link
            if let pw::stream::StreamState::Error(err) = new {
                eprintln!("test_tone: stream error: {err}");
                state_quit.quit();
            }
        })
        .process(move |stream, (step, step_samples, phase)| {
            let mut buffer = match stream.dequeue_buffer() {
                Some(b) => b,
                None => return,
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }
            let stride = SAMPLE_SIZE * n_channels as usize;
            let data = &mut datas[0];

            let n_frames =
                if let Some(slice) = data.data() {
                    let is_tone = *step % 2 == 0;
                    let step_total = if is_tone { tone_samples } else { pause_samples };
                    let remaining = step_total.saturating_sub(*step_samples) as usize;
                    let max_frames = slice.len() / stride;
                    let n = max_frames.min(remaining);
                    let active_ch = if is_tone {
                        sweep_order[*step / 2]
                    } else {
                        usize::MAX
                    };

                    let freq = if is_tone && positions[active_ch] == lfe_id {
                        LFE_FREQ_HZ
                    } else {
                        FREQ_HZ
                    };
                    let phase_step = TWO_PI * freq / SAMPLE_RATE as f64;

                    for i in 0..n {
                        if is_tone {
                            *phase += phase_step;
                            if *phase >= TWO_PI {
                                *phase -= TWO_PI;
                            }
                            let val = (f64::sin(*phase) * AMPLITUDE * i16::MAX as f64) as i16;
                            let bytes = i16::to_le_bytes(val);
                            for c in 0..n_channels as usize {
                                let start = i * stride + c * SAMPLE_SIZE;
                                slice[start..start + SAMPLE_SIZE]
                                    .copy_from_slice(if c == active_ch { &bytes } else { &[0, 0] });
                            }
                        } else {
                            for c in 0..n_channels as usize {
                                let start = i * stride + c * SAMPLE_SIZE;
                                slice[start..start + SAMPLE_SIZE].copy_from_slice(&[0, 0]);
                            }
                        }
                    }

                    *step_samples += n as u32;
                    if *step_samples >= step_total {
                        *step += 1;
                        *step_samples = 0;
                        *phase = 0.0;
                    }
                    if *step >= total_steps {
                        mainloop_quit.quit();
                    }

                    n
                } else {
                    0
                };

            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = stride as _;
            *chunk.size_mut() = (stride * n_frames) as _;
        })
        .register()?;

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
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    // Fallback exit in case the stream doesn't close normally or error
    // Quits the stream after sweep duration + WATCHDOG_MARGIN_S
    let watchdog_loop = mainloop.clone();
    let watchdog = mainloop.loop_().add_timer(move |_| watchdog_loop.quit());
    let _ = watchdog.update_timer(
        Some(std::time::Duration::from_secs_f64(
            sweep_secs + WATCHDOG_MARGIN_S,
        )),
        None,
    );

    mainloop.run();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::pos_str_to_spa_ids;
    use pipewire::spa::sys::*;

    #[test]
    fn maps_known_positions() {
        assert_eq!(
            pos_str_to_spa_ids("FL,FR", 2),
            vec![SPA_AUDIO_CHANNEL_FL, SPA_AUDIO_CHANNEL_FR]
        );
    }

    #[test]
    fn caps_at_channel_count() {
        assert_eq!(
            pos_str_to_spa_ids("FL,FR,FC", 2),
            vec![SPA_AUDIO_CHANNEL_FL, SPA_AUDIO_CHANNEL_FR]
        );
    }

    #[test]
    fn unknown_token_maps_to_unknown() {
        assert_eq!(pos_str_to_spa_ids("ZZ", 1), vec![SPA_AUDIO_CHANNEL_UNKNOWN]);
    }
}
