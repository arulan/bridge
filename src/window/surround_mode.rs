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

// Direct <-> Surround handling for the Main card
// The surround sink is a separate conf-created sink
// When toggled on, the Main card switches to drive it
// and the crossfader, mute, meter, test tone relate to it

use std::path::Path;

use adw::prelude::*;
use adw::subclass::prelude::*;

use super::BridgeWindow;
use crate::audio::backend::PipeWireBackend;
use crate::audio::pw_config::{MAIN_SINK, SURROUND_SINK};
use crate::config::{self, Side};

impl BridgeWindow {
    /// The sink the Main card currently drives, for the default-sink banner
    pub(super) fn active_main_sink(&self) -> &'static str {
        if self.imp().surround_active.get() {
            SURROUND_SINK
        } else {
            MAIN_SINK
        }
    }

    /// If the Virtual Surround sink was already live (from the savec conf)
    /// , this flags the deferred-change banner.
    pub(crate) fn note_surround_reconfig(&self) {
        let imp = self.imp();
        if imp
            .backend
            .borrow()
            .as_ref()
            .is_some_and(|b| b.surround_present())
        {
            imp.surround_pending.set(true);
            // a new change triggers the banner again
            imp.surround_restart_dismissed.set(false);
        }
    }

    /// Show Direct/Surround toggle or Configure button
    pub fn refresh_surround(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        let configured = config::surround_enabled();
        let hrir_ok = configured && hrir_exists();
        let present = backend.surround_present();

        // the toggle appears with a valid surround config (HRIR)
        // shows disabled toggle button until sink is live
        let show_toggle = configured && hrir_ok;

        imp.main_surround_setup_button.set_visible(!configured);
        imp.main_mode_toggle.set_visible(show_toggle);
        imp.main_mode_toggle.set_sensitive(present);
        imp.main_surround_error_banner
            .set_visible(configured && !hrir_ok);

        // There are two banner states:
        // - First setup; sink isn't live yet
        // - Sink is live, but config changes; changes aren't live until relogin
        if !show_toggle {
            imp.surround_pending.set(false);
        }
        let node_absent = show_toggle && !present;
        let reconfig_pending = show_toggle && present && imp.surround_pending.get();
        let pending = node_absent || reconfig_pending;
        if !pending {
            imp.surround_restart_dismissed.set(false);
        }
        imp.main_surround_restart_banner
            .set_visible(pending && !imp.surround_restart_dismissed.get());
        if node_absent {
            imp.main_surround_restart_label
                .set_text("Virtual Surround is available after your next login");
        } else if reconfig_pending {
            imp.main_surround_restart_label
                .set_text("Changes take effect after your next login");
        }

        if imp.surround_active.get() && !(configured && hrir_ok) {
            imp.surround_active.set(false);
            config::set_surround_active(false);
            self.apply_main_mode_swap(&backend, false);
        } else if show_toggle {
            self.force_toggle_to(imp.surround_active.get());
        }
    }

    pub(super) fn on_surround_ready(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };
        self.refresh_surround();

        if imp.surround_active.get() {
            self.force_toggle_to(true);
            self.apply_main_mode_swap(&backend, true);
        } else {
            backend.set_surround_mute(true);
        }
    }

    pub(super) fn on_surround_removed(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };
        // node drops; falls back to direct mode on Main
        if imp.surround_active.get() {
            imp.surround_active.set(false);
            config::set_surround_active(false);
            self.apply_main_mode_swap(&backend, false);
        }
        self.refresh_surround();
    }

    pub(super) fn on_main_mode_toggled(&self, want_surround: bool) {
        let imp = self.imp();
        if imp.mode_swap_in_progress.get() {
            return;
        }
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };
        // can't switch to a surround sink that isn't live yet
        if want_surround && !backend.surround_present() {
            self.force_toggle_to(false);
            return;
        }

        let carry_default = config::default_follows_main()
            && backend.is_default(self.active_main_sink()) == Some(true);

        imp.surround_active.set(want_surround);
        config::set_surround_active(want_surround);
        self.apply_main_mode_swap(&backend, want_surround);

        if carry_default {
            backend.set_default_sink(self.active_main_sink());
        }
    }

    /// Switch the Main card's controls over to the active mode
    fn apply_main_mode_swap(&self, backend: &PipeWireBackend, want_surround: bool) {
        let imp = self.imp();

        let active_muted = if want_surround {
            imp.surround_user_muted.get()
        } else {
            imp.main_muted.get()
        };

        // Inactive side is force-muted; active side restores mute state
        if want_surround {
            backend.set_mute(Side::Main, true);
            backend.set_surround_mute(imp.surround_user_muted.get());
        } else {
            backend.set_surround_mute(true);
            backend.set_mute(Side::Main, imp.main_muted.get());
        }

        imp.mode_swap_in_progress.set(true);
        imp.main_mute_button.set_active(active_muted);
        imp.mode_swap_in_progress.set(false);
        imp.main_mute_image.set_icon_name(Some(if active_muted {
            "speaker-0-symbolic"
        } else {
            "speaker-3-symbolic"
        }));
        imp.main_mute_button.set_tooltip_text(Some(if active_muted {
            "Unmute this output"
        } else {
            "Mute this output"
        }));

        imp.main_subtitle_label.set_label(if want_surround {
            "Bridge - Virtual Surround"
        } else {
            "Bridge - Main"
        });

        // rebuilds the Main dropdown for the mode
        self.populate_dropdowns();
    }

    fn force_toggle_to(&self, want_surround: bool) {
        let imp = self.imp();
        imp.mode_swap_in_progress.set(true);
        imp.main_mode_toggle.set_active_name(Some(if want_surround {
            "surround"
        } else {
            "direct"
        }));
        imp.mode_swap_in_progress.set(false);
    }
}

fn hrir_exists() -> bool {
    let path = config::load_surround().hrir_path;
    !path.is_empty() && Path::new(&path).exists()
}
