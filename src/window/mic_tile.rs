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

// The Bridge - Mic tile

use adw::prelude::*;
use adw::subclass::prelude::*;

use super::BridgeWindow;
use crate::audio::hw_device::{HwDevice, channel_layout_label};
use crate::audio::pw_config;
use crate::audio::role::Role;
use crate::config;
use crate::util::{disconnected_device, hw_device_model, selected_hw_device};

impl BridgeWindow {
    pub(super) fn refresh_mic_tile(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        let sources = backend.hw_sources();
        let configured = config::mic_configured();

        if sources.is_empty() && !configured {
            imp.mic_tile.set_visible(false);
            return;
        }
        imp.mic_tile.set_visible(true);

        if !configured {
            self.show_mic_setup_prompt();
            return;
        }

        let def = config::load_mic();
        let present = sources.iter().any(|s| s.name == def.hw_name);
        self.refresh_mic_dropdown(&sources, &def, present);
        self.refresh_mic_status();
    }

    /// A new device selection should go through this instead of refresh_mic_tile.
    /// Swapping out the model from the dropdown selection notify crashes GTK
    pub(super) fn refresh_mic_status(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        let def = config::load_mic();
        let sources = backend.hw_sources();
        let device = sources.iter().find(|s| s.name == def.hw_name);
        let present = device.is_some();
        imp.mic_disconnected.set(!present);

        imp.mic_hw_dropdown.set_visible(true);
        imp.mic_status_row.set_visible(true);
        imp.mic_mode_toggle.set_visible(true);
        imp.mic_setup_banner.set_visible(false);

        let status = match device {
            Some(d) => d.status_label(),
            None => channel_layout_label(def.channels, &def.position),
        };
        imp.mic_channels_label.set_label(&status);

        let live = backend.present(Role::Mic);
        let unavailable = backend.mic_unavailable();

        if unavailable {
            imp.mic_error_label.set_label("Bridge - Mic is unavailable");
        } else if !present {
            imp.mic_error_label.set_label("Microphone disconnected");
        }
        imp.mic_error_banner.set_visible(unavailable || !present);

        imp.mic_mode_toggle.set_sensitive(live);
        imp.mic_hw_dropdown.set_sensitive(live);

        self.refresh_mic_state();
        self.refresh_mic_default_banner();
    }

    fn refresh_mic_state(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        let label = &imp.mic_state_label;
        label.set_visible(backend.present(Role::Mic));

        if imp.mic_muted.get() {
            label.set_label("Muted");
            label.remove_css_class("live");
            label.add_css_class("idle");
        } else {
            label.set_label("Live");
            label.remove_css_class("idle");
            label.add_css_class("live");
        }
    }

    fn show_mic_setup_prompt(&self) {
        let imp = self.imp();
        imp.mic_setup_banner.set_visible(true);
        imp.mic_hw_dropdown.set_visible(false);
        imp.mic_status_row.set_visible(false);
        imp.mic_mode_toggle.set_visible(false);
        imp.mic_state_label.set_visible(false);
        imp.mic_error_banner.set_visible(false);
        imp.mic_default_banner.set_visible(false);
        imp.mic_default_tag.set_visible(false);
        imp.mic_level_bar.set_value(0.0);
    }

    fn refresh_mic_dropdown(&self, sources: &[HwDevice], def: &config::SinkDef, present: bool) {
        let imp = self.imp();
        let model = hw_device_model(sources);
        if !present {
            model.insert(0, &glib::BoxedAnyObject::new(disconnected_device(def)));
        }

        let idx = if present {
            sources
                .iter()
                .position(|s| s.name == def.hw_name)
                .unwrap_or(0) as u32
        } else {
            0
        };

        imp.suppress_selected.set(true);
        imp.mic_hw_dropdown.set_model(Some(&model));
        imp.mic_hw_dropdown.set_selected(idx);
        imp.suppress_selected.set(false);
    }

    pub(super) fn refresh_mic_default_banner(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        let is_default = backend.mic_is_default();
        imp.mic_default_tag
            .set_visible(is_default == Some(true) && config::mic_configured());

        let offer =
            is_default == Some(false) && config::mic_configured() && backend.present(Role::Mic);
        imp.mic_default_banner.set_visible(offer);
    }

    pub(super) fn on_mic_selected(&self) {
        let imp = self.imp();
        if imp.suppress_selected.get() {
            return;
        }

        let Some(device) = selected_hw_device(&imp.mic_hw_dropdown) else {
            return;
        };
        // disconnected placeholder
        if device.node_id == 0 {
            return;
        }

        let was_disconnected = imp.mic_disconnected.get();
        let def: config::SinkDef = device.into();
        config::store_mic(&def);
        if let Err(e) = pw_config::write_mic_config(&def) {
            eprintln!("mic: failed to persist config: {e}");
        }

        if let Some(backend) = imp.backend.borrow().clone() {
            backend.retarget(Role::Mic, &def.hw_name);
        }

        if was_disconnected {
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = w)]
                self,
                move || w.refresh_mic_tile()
            ));
            return;
        }

        self.refresh_mic_status();
    }

    pub(super) fn on_mic_mode_toggled(&self, muted: bool) {
        let imp = self.imp();
        imp.mic_muted.set(muted);
        if let Some(backend) = imp.backend.borrow().clone() {
            backend.set_mute(Role::Mic, muted);
        }
        self.refresh_mic_state();
    }

    pub(super) fn on_mic_ready(&self) {
        let imp = self.imp();
        let muted = imp.mic_muted.get();
        if let Some(backend) = imp.backend.borrow().clone() {
            backend.set_mute(Role::Mic, muted);
        }
        self.refresh_mic_tile();
    }

    pub(super) fn on_mic_removed(&self) {
        self.refresh_mic_tile();
    }

    pub(super) fn on_mic_failed(&self) {
        self.refresh_mic_tile();
    }

    pub(super) fn set_mic_meter_enabled(&self, enabled: bool) {
        if let Some(backend) = self.imp().backend.borrow().clone() {
            backend.set_mic_meter(enabled);
        }
        if !enabled {
            self.imp().mic_level_bar.set_value(0.0);
        }
    }
}
