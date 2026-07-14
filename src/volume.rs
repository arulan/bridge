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
            VolumeDisplay::Decibel => "db",
            VolumeDisplay::Percentage => "percent",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "percent" => VolumeDisplay::Percentage,
            _ => VolumeDisplay::Decibel,
        }
    }

    pub fn load() -> Self {
        Self::from_key(&settings().string("volume-display"))
    }

    pub fn store(self) {
        let _ = settings().set_string("volume-display", self.as_key());
    }

    pub fn format_parts(self, mul: f64) -> (String, &'static str) {
        match self {
            VolumeDisplay::Decibel => {
                let db = if (mul - 1.0).abs() < f64::EPSILON {
                    "0".to_string()
                } else if mul <= 0.0 {
                    "−∞".to_string() // minus sign + infinity
                } else {
                    format!("{:.1}", 20.0 * mul.log10())
                };
                (db, "dB")
            }
            VolumeDisplay::Percentage => (format!("{:.0}", (mul * 100.0).round()), "%"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_parts() {
        assert_eq!(VolumeDisplay::Decibel.format_parts(1.0), ("0".into(), "dB"));
        let half = 10f64.powf(-15.0 / 20.0);
        assert_eq!(
            VolumeDisplay::Decibel.format_parts(half),
            ("-15.0".into(), "dB")
        );
        assert_eq!(
            VolumeDisplay::Decibel.format_parts(0.0),
            ("−∞".into(), "dB")
        );
    }

    #[test]
    fn percent_parts() {
        assert_eq!(
            VolumeDisplay::Percentage.format_parts(1.0),
            ("100".into(), "%")
        );
        assert_eq!(
            VolumeDisplay::Percentage.format_parts(0.5),
            ("50".into(), "%")
        );
        assert_eq!(
            VolumeDisplay::Percentage.format_parts(0.0),
            ("0".into(), "%")
        );
    }
}
