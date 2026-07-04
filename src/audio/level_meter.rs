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

// Per-sink peak levels from the capture streams on the pw_connection thread

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::config::Side;

/// One peak atomic per virtual sink.
/// Each holds the loudest sample seen since the last read.
pub struct LevelMeters {
    aux: Arc<AtomicU32>,
    main: Arc<AtomicU32>,
}

impl Default for LevelMeters {
    fn default() -> Self {
        Self::new()
    }
}

impl LevelMeters {
    pub fn new() -> Self {
        LevelMeters {
            aux: Arc::new(AtomicU32::new(0)),
            main: Arc::new(AtomicU32::new(0)),
        }
    }

    /// clones of the (aux, main) atomics that the capture streams write into
    pub fn atoms(&self) -> (Arc<AtomicU32>, Arc<AtomicU32>) {
        (Arc::clone(&self.aux), Arc::clone(&self.main))
    }

    /// Peak observed since the last call
    pub fn peak(&self, side: Side) -> f32 {
        let atomic = match side {
            Side::Aux => &self.aux,
            Side::Main => &self.main,
        };
        take_peak(atomic)
    }
}

/// Peak held in the atomic since the last read, resetting it
pub(crate) fn take_peak(atomic: &AtomicU32) -> f32 {
    f32::from_bits(atomic.swap(0, Ordering::Relaxed))
}

pub(crate) fn peak_f32le(slice: &[u8]) -> f32 {
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
