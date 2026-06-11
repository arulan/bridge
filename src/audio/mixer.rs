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

// Mixer goes from -30dB --- 0 --- -30dB 
// Slider set to half-way left  produces <  0dB Aux -|- + --- -15dB Main>
// Slider set to half-way right produces <-15dB Aux --- + -|-   0dB Main>
// Slider set to middle produces         <  0dB Aux --- | ---   0dB Main>
// Extreme left and right positions produces Mute in the other sink
const FLOOR_DB: f64 = -30.0;

fn position_to_multiplier(t: f64) -> f64 {
    if t <= 0.0 { return 1.0; }
    if t >= 1.0 { return 0.0; }
    10f64.powf(t * FLOOR_DB / 20.0)
}

/// Returns (aux_multiplier, main_multiplier)
pub fn calculate_multipliers(v: f64) -> (f64, f64) {
    let aux  = position_to_multiplier(f64::max(0.0,  v));
    let main = position_to_multiplier(f64::max(0.0, -v));
    (aux, main)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_middle() {
        assert_eq!(calculate_multipliers(0.0), (1.0, 1.0));
    }

    #[test]
    fn extremes_mute() {
        assert_eq!(calculate_multipliers(1.0),  (0.0, 1.0));
        assert_eq!(calculate_multipliers(-1.0), (1.0, 0.0));
    }

    #[test]
    fn half_way_half_floor() {
        // FLOOR_DB / 2 = -15dB at half way left or right
        let (aux, main) = calculate_multipliers(0.5);
        assert!((aux - 10f64.powf(-15.0 / 20.0)).abs() < 1e-9);
        assert_eq!(main, 1.0);
    }
}
