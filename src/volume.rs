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

use gio::prelude::*;

use crate::application::settings;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum VolumeDisplay {
    #[default]
    Decibel,
    Percentage,
}

impl VolumeDisplay {
    pub fn as_key(self) -> &'static str {
        match self {
            VolumeDisplay::Decibel    => "db",
            VolumeDisplay::Percentage => "percent",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "percent" => VolumeDisplay::Percentage,
            _         => VolumeDisplay::Decibel,
        }
    }

    pub fn load() -> Self {
        Self::from_key(&settings().string("volume-display"))
    }

    pub fn store(self) {
        let _ = settings().set_string("volume-display", self.as_key());
    }

    pub fn format(self, mul: f64) -> String {
        match self {
            VolumeDisplay::Decibel    => format_db(mul),
            VolumeDisplay::Percentage => format_percent(mul),
        }
    }
}

fn format_db(mul: f64) -> String {
    if mul <= 0.0 { return "Muted".into(); }
    if (mul - 1.0).abs() < f64::EPSILON { return "0 dB".into(); }
    
    // negative only
    format!("-{:.1} dB", -(20.0 * mul.log10()))
}

fn format_percent(mul: f64) -> String {
    if mul <= 0.0 { return "Muted".into(); }
    format!("{:.0}%", (mul * 100.0).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_readout() {
        assert_eq!(format_db(1.0), "0 dB");
        assert_eq!(format_db(0.0), "Muted");
        let half = 10f64.powf(-15.0 / 20.0);
        assert_eq!(format_db(half), "-15.0 dB");
    }

    #[test]
    fn percent_readout() {
        assert_eq!(format_percent(1.0), "100%");
        assert_eq!(format_percent(0.0), "Muted");
        assert_eq!(format_percent(0.5), "50%");
    }
}
