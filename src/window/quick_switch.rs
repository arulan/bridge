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

// Quick Switch: toggle the two output presets from the header

use adw::prelude::*;
use adw::subclass::prelude::*;

use super::BridgeWindow;
use crate::audio::hw_sink::HwSink;
use crate::config::{self, Side};

impl BridgeWindow {
    pub fn update_qs_toggle(&self) {
        let imp = self.imp();

        if !config::presets_configured() {
            imp.qs_switch_button.set_visible(false);
            imp.qs_configure_button.set_visible(true);
            return;
        }

        imp.qs_configure_button.set_visible(false);
        imp.qs_switch_button.set_visible(true);

        if imp.surround_active.get() {
            self.qs_disable(
                "Unavailable in Surround",
                "Quick Switch is unavailable in Virtual Surround",
            );
            return;
        }

        let presets = config::load_presets();
        let (Some(a), Some(b)) = (presets.first(), presets.get(1)) else {
            return;
        };

        let cfg = config::load();
        let aux_hw = cfg.aux.hw_name;
        let main_hw = cfg.main.hw_name;

        let name_a = if a.name.is_empty() { "A" } else { &a.name };
        let name_b = if b.name.is_empty() { "B" } else { &b.name };

        let (target, icon, label, tooltip) = if a.matches(&aux_hw, &main_hw) {
            (
                b,
                "horizontal-arrows-symbolic",
                format!("Switch to {name_b}"),
                format!("Currently on {name_a}"),
            )
        } else if b.matches(&aux_hw, &main_hw) {
            (
                a,
                "horizontal-arrows-symbolic",
                format!("Switch to {name_a}"),
                format!("Currently on {name_b}"),
            )
        } else {
            (
                a,
                "arrow-turn-right-horizontal2-symbolic",
                format!("Switch to {name_a}"),
                "Current outputs match no preset".to_owned(),
            )
        };

        // hardware unavailable/disconnected path
        let sinks = imp
            .backend
            .borrow()
            .as_ref()
            .map(|be| be.hw_sinks())
            .unwrap_or_default();
        if !preset_devices_present(target, &sinks) {
            let target_name = if target.name.is_empty() {
                "The target preset"
            } else {
                &target.name
            };
            self.qs_disable(
                "Output Unavailable",
                &format!("{target_name} uses a disconnected output"),
            );
            return;
        }

        imp.qs_switch_content.set_icon_name(icon);
        imp.qs_switch_content.set_label(&label);
        imp.qs_switch_button.set_tooltip_text(Some(&tooltip));
        imp.qs_switch_button.set_sensitive(true);
    }

    fn qs_disable(&self, label: &str, tooltip: &str) {
        let imp = self.imp();
        imp.qs_switch_content
            .set_icon_name("horizontal-arrows-disabled-symbolic");
        imp.qs_switch_content.set_label(label);
        imp.qs_switch_button.set_tooltip_text(Some(tooltip));
        imp.qs_switch_button.set_sensitive(false);
    }

    // switch to the other preset; matching neither jumps to the first
    pub(super) fn quick_switch_execute(&self) {
        let imp = self.imp();
        if imp.surround_active.get() {
            return;
        }
        if !config::presets_configured() {
            return;
        }

        let presets = config::load_presets();
        let (Some(a), Some(b)) = (presets.first(), presets.get(1)) else {
            return;
        };

        let cfg = config::load();
        let aux_hw = cfg.aux.hw_name;
        let main_hw = cfg.main.hw_name;

        let target = if a.matches(&aux_hw, &main_hw) { b } else { a };

        let sinks = imp
            .backend
            .borrow()
            .as_ref()
            .map(|be| be.hw_sinks())
            .unwrap_or_default();
        if !preset_devices_present(target, &sinks) {
            return;
        }

        if !target.aux_hw.is_empty() {
            self.select_side_hw(Side::Aux, &target.aux_hw);
        }
        if !target.main_hw.is_empty() {
            self.select_side_hw(Side::Main, &target.main_hw);
        }
    }

    fn select_side_hw(&self, side: Side, hw_name: &str) {
        let imp = self.imp();
        let dropdown = match side {
            Side::Aux => &*imp.aux_hw_dropdown,
            Side::Main => &*imp.main_hw_dropdown,
        };
        let Some(model) = dropdown.model() else {
            return;
        };

        for i in 0..model.n_items() {
            let Some(sink) = model
                .item(i)
                .and_downcast::<glib::BoxedAnyObject>()
                .map(|b| b.borrow::<HwSink>().clone())
            else {
                continue;
            };
            if sink.node_id != 0 && sink.name == hw_name {
                dropdown.set_selected(i);
                return;
            }
        }
    }
}

fn preset_devices_present(preset: &config::Preset, sinks: &[HwSink]) -> bool {
    let present = |name: &str| name.is_empty() || sinks.iter().any(|s| s.name == name);
    present(&preset.aux_hw) && present(&preset.main_hw)
}
