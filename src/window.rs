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

mod routing_tile;
mod stream_list;
mod surround_mode;

use std::cell::{Cell, OnceCell, RefCell};
use std::time::Duration;

use adw::subclass::prelude::*;
use glib::subclass::InitializingObject;
use gtk4::{self as gtk, CompositeTemplate, prelude::*};

use crate::audio::backend::PipeWireBackend;
use crate::audio::hw_sink::HwSink;
use crate::audio::{mixer, pw_config};
use crate::config::{self, Side};
use crate::shortcuts::ShortcutsPortal;
use crate::util::{
    drive_stream_meters, hw_sink_factory, hw_sink_model, selected_hw_sink, stream_count,
};
use crate::volume::VolumeDisplay;

#[derive(CompositeTemplate, Default)]
#[template(resource = "/io/github/arulan/Dashboard/ui/window.ui")]
pub struct DashboardWindowImp {
    #[template_child]
    pub persist_banner: TemplateChild<adw::Banner>,
    #[template_child]
    pub shortcuts_banner: TemplateChild<adw::Banner>,
    #[template_child]
    pub aux_hw_dropdown: TemplateChild<gtk::DropDown>,
    #[template_child]
    pub main_hw_dropdown: TemplateChild<gtk::DropDown>,
    #[template_child]
    pub aux_mute_button: TemplateChild<gtk::ToggleButton>,
    #[template_child]
    pub main_mute_button: TemplateChild<gtk::ToggleButton>,
    #[template_child]
    pub aux_mute_image: TemplateChild<gtk::Image>,
    #[template_child]
    pub main_mute_image: TemplateChild<gtk::Image>,
    #[template_child]
    pub aux_streams_button: TemplateChild<gtk::MenuButton>,
    #[template_child]
    pub aux_streams_label: TemplateChild<gtk::Label>,
    #[template_child]
    pub aux_test_tone_button: TemplateChild<gtk::Button>,
    #[template_child]
    pub main_test_tone_button: TemplateChild<gtk::Button>,
    #[template_child]
    pub aux_level_bar: TemplateChild<gtk::LevelBar>,
    #[template_child]
    pub main_level_bar: TemplateChild<gtk::LevelBar>,
    #[template_child]
    pub aux_channels_label: TemplateChild<gtk::Label>,
    #[template_child]
    pub main_channels_label: TemplateChild<gtk::Label>,
    #[template_child]
    pub aux_status_row: TemplateChild<gtk::Box>,
    #[template_child]
    pub main_status_row: TemplateChild<gtk::Box>,
    #[template_child]
    pub mix_scale: TemplateChild<gtk::Scale>,
    #[template_child]
    pub aux_volume_box: TemplateChild<gtk::Box>,
    #[template_child]
    pub main_volume_box: TemplateChild<gtk::Box>,
    #[template_child]
    pub aux_volume_value: TemplateChild<gtk::Label>,
    #[template_child]
    pub main_volume_value: TemplateChild<gtk::Label>,
    #[template_child]
    pub aux_volume_unit: TemplateChild<gtk::Label>,
    #[template_child]
    pub main_volume_unit: TemplateChild<gtk::Label>,
    #[template_child]
    pub main_default_banner: TemplateChild<gtk::Box>,
    #[template_child]
    pub main_default_button: TemplateChild<gtk::Button>,
    #[template_child]
    pub main_default_tag: TemplateChild<gtk::Label>,
    #[template_child]
    pub main_subtitle_label: TemplateChild<gtk::Label>,
    #[template_child]
    pub main_mode_toggle: TemplateChild<adw::ToggleGroup>,
    #[template_child]
    pub main_surround_setup_button: TemplateChild<gtk::Button>,
    #[template_child]
    pub main_surround_restart_banner: TemplateChild<gtk::Box>,
    #[template_child]
    pub main_surround_restart_label: TemplateChild<gtk::Label>,
    #[template_child]
    pub main_surround_restart_dismiss: TemplateChild<gtk::Button>,
    #[template_child]
    pub main_surround_error_banner: TemplateChild<gtk::Box>,
    #[template_child]
    pub aux_disconnect_banner: TemplateChild<gtk::Box>,
    #[template_child]
    pub main_disconnect_banner: TemplateChild<gtk::Box>,
    #[template_child]
    pub routing_header_list: TemplateChild<gtk::ListBox>,
    #[template_child]
    pub routing_toggle: TemplateChild<gtk::ListBoxRow>,
    #[template_child]
    pub routing_add_button: TemplateChild<gtk::Button>,
    #[template_child]
    pub routing_badge: TemplateChild<gtk::Label>,
    #[template_child]
    pub routing_chevron: TemplateChild<gtk::Image>,
    #[template_child]
    pub routing_revealer: TemplateChild<gtk::Revealer>,
    #[template_child]
    pub routing_body: TemplateChild<gtk::Box>,

    backend: RefCell<Option<PipeWireBackend>>,
    suppress_selected: Cell<bool>,
    aux_disconnected: Cell<bool>,
    main_disconnected: Cell<bool>,

    // Virtual Surround is a mode that changes the virtual sink driving the Main card
    surround_active: Cell<bool>,
    main_muted: Cell<bool>,
    surround_user_muted: Cell<bool>,
    mode_swap_in_progress: Cell<bool>,
    // conf was rewritten while the virtual surround node was live; applies next login
    surround_pending: Cell<bool>,
    // hides the restart banner until a new pending change triggers it
    surround_restart_dismissed: Cell<bool>,

    shortcut_banner_dismissed: Cell<bool>,

    routing_row_meters: RefCell<Vec<(gtk::LevelBar, Vec<u32>)>>,

    // set while the Add Rule dialog is open
    // the Add Rule dialog meters drain the same swap-to-zero peaks from the routing tile
    stream_meters_paused: Cell<bool>,

    routing_size_groups: RefCell<Vec<gtk::SizeGroup>>,

    volume_display: Cell<VolumeDisplay>,
    // crossfader step as the delta value on [-1, 1]
    crossfade_step: Cell<f64>,
    settings: OnceCell<gio::Settings>,

    activity_tick_id: RefCell<Option<glib::SourceId>>,
    scale_css: RefCell<Option<gtk::CssProvider>>,

    aux_streams_list: RefCell<Option<gtk::ListBox>>,
}

#[glib::object_subclass]
impl ObjectSubclass for DashboardWindowImp {
    const NAME: &'static str = "DashboardWindow";
    type Type = DashboardWindow;
    type ParentType = adw::ApplicationWindow;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for DashboardWindowImp {}
impl WidgetImpl for DashboardWindowImp {}
impl WindowImpl for DashboardWindowImp {}
impl ApplicationWindowImpl for DashboardWindowImp {}
impl AdwApplicationWindowImpl for DashboardWindowImp {}

glib::wrapper! {
    pub struct DashboardWindow(ObjectSubclass<DashboardWindowImp>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap,
                    gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
                    gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl DashboardWindow {
    pub fn new(app: &adw::Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    pub fn setup(&self, backend: &PipeWireBackend) {
        let imp = self.imp();

        // TODO: Look into GResource later
        add_css();

        self.restore_window_state();
        self.connect_close_request(glib::clone!(
            #[weak(rename_to = w)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_| {
                w.save_window_state();
                glib::Propagation::Proceed
            }
        ));

        // Override slider fill bheavior (center -> selection)
        if let Some(display) = gtk::gdk::Display::default() {
            let provider = gtk::CssProvider::new();
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            imp.scale_css.replace(Some(provider));
        }
        imp.mix_scale.add_css_class("mix-crossfader");
        self.render_fill(imp.mix_scale.value());

        // meter stays uniform color
        for bar in [imp.aux_level_bar.get(), imp.main_level_bar.get()] {
            bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_LOW));
            bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_HIGH));
            bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_FULL));
            bar.add_css_class("level-meter");
        }

        imp.aux_hw_dropdown.set_factory(Some(&hw_sink_factory()));
        imp.main_hw_dropdown.set_factory(Some(&hw_sink_factory()));

        imp.aux_hw_dropdown.connect_selected_notify(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.on_hw_selected(Side::Aux)
        ));
        imp.main_hw_dropdown.connect_selected_notify(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.on_hw_selected(Side::Main)
        ));

        imp.persist_banner.connect_button_clicked(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.imp().persist_banner.set_revealed(false)
        ));

        imp.mix_scale.connect_value_changed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.apply_mix()
        ));

        // Double-click the crossfade mixer to return to balanced (0.0)
        let reset_gesture = gtk::GestureClick::new();
        reset_gesture.set_button(gtk::gdk::BUTTON_PRIMARY);
        reset_gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        reset_gesture.connect_pressed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |g, n_press, _, _| {
                if n_press == 2 {
                    w.imp().mix_scale.set_value(0.0);
                    g.set_state(gtk::EventSequenceState::Claimed);
                }
            }
        ));
        imp.mix_scale.add_controller(reset_gesture);

        imp.aux_mute_button.connect_toggled(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |b| w.on_mute_toggled(Side::Aux, b.is_active())
        ));
        imp.main_mute_button.connect_toggled(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |b| w.on_mute_toggled(Side::Main, b.is_active())
        ));

        imp.aux_test_tone_button.connect_clicked(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.on_test_clicked(Side::Aux)
        ));
        imp.main_test_tone_button.connect_clicked(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.on_test_clicked(Side::Main)
        ));

        let (aux_popover, aux_list) = stream_list::streams_popover("Streams on Aux");
        imp.aux_streams_button.set_popover(Some(&aux_popover));
        imp.aux_streams_list.replace(Some(aux_list));
        aux_popover.connect_map(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.populate_aux_streams_list()
        ));

        imp.volume_display.set(VolumeDisplay::load());
        self.apply_crossfade_step();
        let settings = crate::application::settings();
        settings.connect_changed(
            Some("volume-display"),
            glib::clone!(
                #[weak(rename_to = w)]
                self,
                move |_, _| {
                    w.imp().volume_display.set(VolumeDisplay::load());
                    w.update_readout_labels();
                }
            ),
        );
        settings.connect_changed(
            Some("crossfade-step"),
            glib::clone!(
                #[weak(rename_to = w)]
                self,
                move |_, _| w.apply_crossfade_step()
            ),
        );
        let _ = imp.settings.set(settings);
        self.update_readout_labels();

        imp.main_default_button.connect_clicked(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| {
                let Some(backend) = w.imp().backend.borrow().clone() else {
                    return;
                };
                backend.set_default_sink(w.active_main_sink());
            }
        ));

        // virtual surround is persist only
        imp.surround_active.set(config::surround_active());

        imp.main_mode_toggle
            .connect_active_name_notify(glib::clone!(
                #[weak(rename_to = w)]
                self,
                move |tg| {
                    let want_surround = tg.active_name().as_deref() == Some("surround");
                    w.on_main_mode_toggled(want_surround);
                }
            ));

        imp.main_surround_setup_button.connect_clicked(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| {
                let _ = gtk::prelude::WidgetExt::activate_action(&w, "app.surround", None);
            }
        ));

        imp.main_surround_restart_dismiss.connect_clicked(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| {
                w.imp().surround_restart_dismissed.set(true);
                w.imp().main_surround_restart_banner.set_visible(false);
            }
        ));

        backend.connect_surround_ready(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.on_surround_ready()
        ));

        // the node dropping falls back to direct mode
        backend.connect_surround_removed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.on_surround_removed()
        ));

        imp.routing_header_list.connect_row_activated(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_, _| w.toggle_routing_expanded()
        ));

        imp.routing_add_button.connect_clicked(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.show_add_rule_dialog(&[])
        ));

        backend.connect_sinks_ready(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| {
                w.populate_dropdowns();
                w.refresh_routing_tile();
                w.refresh_aux_stream_button();
            }
        ));

        backend.connect_sinks_changed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| {
                w.populate_dropdowns();
                w.refresh_routing_tile();
            }
        ));

        backend.connect_streams_changed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.refresh_routing_tile()
        ));

        backend.connect_aux_streams_changed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.refresh_aux_stream_button()
        ));

        backend.connect_default_changed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.refresh_default_banner()
        ));

        backend.connect_owned_changed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.sync_controls()
        ));
        *imp.backend.borrow_mut() = Some(backend.clone());

        self.set_routing_expanded(config::keep_routing_open());
        self.refresh_surround();
        self.start_activity_ticker();
    }

    fn restore_window_state(&self) {
        let settings = crate::application::settings();
        self.set_default_size(settings.int("window-width"), settings.int("window-height"));
        if settings.boolean("is-maximized") {
            self.maximize();
        }
    }

    fn save_window_state(&self) {
        let settings = crate::application::settings();
        let (width, height) = self.default_size();
        let _ = settings.set_int("window-width", width);
        let _ = settings.set_int("window-height", height);
        let _ = settings.set_boolean("is-maximized", self.is_maximized());
    }

    // Target ~40ms to avoid running into the PW quantum, causing ghosting/flickering
    // TODO: Should we calculate the quantum for low-latency users and increase our tick?
    fn start_activity_ticker(&self) {
        let id = glib::timeout_add_local(
            Duration::from_millis(40),
            glib::clone!(
                #[weak(rename_to = w)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    w.on_activity_tick();
                    glib::ControlFlow::Continue
                }
            ),
        );
        *self.imp().activity_tick_id.borrow_mut() = Some(id);
    }

    fn on_activity_tick(&self) {
        const SMOOTHING: f64 = 0.3;
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        let surround_active = imp.surround_active.get();
        for (side, bar, mute_btn) in [
            (Side::Aux, &*imp.aux_level_bar, &*imp.aux_mute_button),
            (Side::Main, &*imp.main_level_bar, &*imp.main_mute_button),
        ] {
            let val = if mute_btn.is_active() {
                0.0
            } else {
                // in surround mode the Main level meter follows the surround sink
                let peak = if side == Side::Main && surround_active {
                    backend.surround_peak()
                } else {
                    backend.peak(side)
                } as f64;
                (peak * SMOOTHING + bar.value() * (1.0 - SMOOTHING)).clamp(0.0, 1.0)
            };
            bar.set_value(val);
        }

        if !imp.stream_meters_paused.get() {
            drive_stream_meters(&backend, &imp.routing_row_meters.borrow());
        }
    }

    pub fn populate_dropdowns(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        let sinks = backend.hw_sinks();
        let cfg = config::load();

        // guard against set_model & set_selected firing notify::selected
        // when user hasn't changed hw_dropdown
        imp.suppress_selected.set(true);
        self.refresh_side_dropdown(Side::Aux, &sinks, &cfg);
        self.refresh_side_dropdown(Side::Main, &sinks, &cfg);
        imp.suppress_selected.set(false);

        self.refresh_channels_label(Side::Aux);
        self.refresh_channels_label(Side::Main);

        self.sync_controls();
    }

    // Rebuild one side's dropdown model and selection
    // Prepend "Disconnected —" when selected hw device disconnects
    fn refresh_side_dropdown(&self, side: Side, sinks: &[HwSink], cfg: &config::SinkConfig) {
        let imp = self.imp();
        let (dropdown, banner, disc_cell) = match side {
            Side::Aux => (
                &*imp.aux_hw_dropdown,
                &*imp.aux_disconnect_banner,
                &imp.aux_disconnected,
            ),
            Side::Main => (
                &*imp.main_hw_dropdown,
                &*imp.main_disconnect_banner,
                &imp.main_disconnected,
            ),
        };

        // in surround mode the hw output dropdown is disabled
        // it's configured in the setup dialog
        let surround_lock = side == Side::Main && imp.surround_active.get();
        let surround_def;
        let def = if surround_lock {
            let sc = config::load_surround();
            surround_def = config::SinkDef {
                channels: 2,
                position: "FL,FR".to_owned(),
                hw_name: sc.hw_name,
                display_name: sc.display_name,
            };
            &surround_def
        } else {
            cfg.side(side)
        };
        let present = sinks.iter().any(|s| s.name == def.hw_name);
        let disconnected = !def.hw_name.is_empty() && !present;

        let model = hw_sink_model(sinks);
        if disconnected {
            let label = if def.display_name.is_empty() {
                "Disconnected".to_owned()
            } else {
                format!("Disconnected — {}", def.display_name)
            };
            let placeholder = HwSink {
                node_id: 0,
                name: def.hw_name.clone(),
                display_name: label,
                device_api: String::new(),
                device_bus: String::new(),
                profile_name: String::new(),
                channels: def.channels,
                position: def.position.clone(),
            };
            model.insert(0, &glib::BoxedAnyObject::new(placeholder));
        }
        dropdown.set_model(Some(&model));

        let idx = if disconnected {
            0
        } else {
            sinks
                .iter()
                .position(|s| s.name == def.hw_name)
                .unwrap_or(0) as u32
        };
        dropdown.set_selected(idx);

        disc_cell.set(disconnected);
        banner.set_visible(disconnected);

        // the surround output can only be changed from its dialog
        if side == Side::Main {
            dropdown.set_sensitive(!surround_lock);
            dropdown.set_tooltip_text(surround_lock.then_some("Configured in Virtual Surround"));
        }
    }

    fn sync_controls(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        // controls are disabled until our virtual sinks exist
        // controls are disabled if its hw output is disconnected
        let present = backend.owned_sinks_present();
        let aux_disc = imp.aux_disconnected.get();
        let main_disc = imp.main_disconnected.get();

        imp.aux_mute_button.set_sensitive(present && !aux_disc);
        imp.main_mute_button.set_sensitive(present && !main_disc);
        imp.aux_test_tone_button.set_sensitive(present && !aux_disc);
        imp.main_test_tone_button
            .set_sensitive(present && !main_disc);

        // disable crossfading when either side's hw is disconnected
        imp.mix_scale
            .set_sensitive(present && !aux_disc && !main_disc);

        if present {
            self.apply_mix();
            backend.set_mute(Side::Aux, imp.aux_mute_button.is_active());
            if imp.surround_active.get() {
                backend.set_mute(Side::Main, true);
                backend.set_surround_mute(imp.main_mute_button.is_active());
            } else {
                backend.set_mute(Side::Main, imp.main_mute_button.is_active());
                if backend.surround_present() {
                    backend.set_surround_mute(true);
                }
            }
        }

        let persistent = present && !backend.using_temp_sinks();

        // only display when persistent virtual sinks aren't live yet
        // temp sinks are only live while the app is open
        imp.persist_banner
            .set_revealed(config::is_configured() && !persistent);

        // keep the default banner/tag + surround in step
        self.refresh_default_banner();
        self.refresh_surround();
    }

    fn apply_mix(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        let (aux, main) = mixer::calculate_multipliers(imp.mix_scale.value());

        backend.set_volume(Side::Aux, aux);
        backend.set_volume(Side::Main, main);

        if backend.surround_present() {
            backend.set_surround_volume(main);
        }

        self.render_fill(imp.mix_scale.value());
        self.update_readout_labels();
    }

    pub fn bind_shortcuts(&self, portal: &ShortcutsPortal) {
        let imp = self.imp();

        imp.shortcuts_banner.connect_button_clicked(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |b| {
                w.imp().shortcut_banner_dismissed.set(true);
                b.set_revealed(false);
            }
        ));

        portal.connect_active_changed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |p| w.update_shortcuts_banner(p.is_active())
        ));

        portal.connect_shortcut_activated(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_, id| match id {
                "step-left" => w.step(-1.0),
                "step-right" => w.step(1.0),
                "reset" => w.imp().mix_scale.set_value(0.0),
                _ => {}
            }
        ));
    }

    fn step(&self, dir: f64) {
        let scale = &self.imp().mix_scale;
        scale.set_value(scale.value() + dir * self.imp().crossfade_step.get());
    }

    fn apply_crossfade_step(&self) {
        let delta = config::crossfade_step() as f64 / 50.0;
        self.imp().crossfade_step.set(delta);
        self.imp().mix_scale.adjustment().set_step_increment(delta);
    }

    fn update_shortcuts_banner(&self, active: bool) {
        let imp = self.imp();

        if active {
            imp.shortcut_banner_dismissed.set(false);
            imp.shortcuts_banner.set_revealed(false);
        } else if !imp.shortcut_banner_dismissed.get() {
            imp.shortcuts_banner.set_revealed(true);
        }
    }

    // fill bheavior center -> selection
    fn render_fill(&self, v: f64) {
        let imp = self.imp();
        let Some(provider) = imp.scale_css.borrow().clone() else {
            return;
        };

        let pct = (v + 1.0) / 2.0 * 100.0;
        let lo = f64::min(50.0, pct);
        let hi = f64::max(50.0, pct);
        provider.load_from_string(&format!(
            "scale.mix-crossfader trough highlight {{ background: transparent; box-shadow: none; transition: none; }}\n\
             scale.mix-crossfader trough {{ transition: none; background-image: linear-gradient(to right, \
               transparent {lo:.2}%, @accent_bg_color {lo:.2}%, \
               @accent_bg_color {hi:.2}%, transparent {hi:.2}%); }}"
        ));
    }

    fn on_mute_toggled(&self, side: Side, muted: bool) {
        let imp = self.imp();

        if imp.mode_swap_in_progress.get() {
            return;
        }

        let (img, btn) = match side {
            Side::Aux => (&*imp.aux_mute_image, &*imp.aux_mute_button),
            Side::Main => (&*imp.main_mute_image, &*imp.main_mute_button),
        };
        img.set_icon_name(Some(if muted {
            "speaker-0-symbolic"
        } else {
            "speaker-3-symbolic"
        }));
        btn.set_tooltip_text(Some(if muted {
            "Unmute this output"
        } else {
            "Mute this output"
        }));

        if let Some(backend) = imp.backend.borrow().clone() {
            if side == Side::Main && imp.surround_active.get() {
                imp.surround_user_muted.set(muted);
                backend.set_surround_mute(muted);
            } else {
                if side == Side::Main {
                    imp.main_muted.set(muted);
                }
                backend.set_mute(side, muted);
            }
        }

        self.update_readout_labels();
    }

    fn on_test_clicked(&self, side: Side) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        let btn = match side {
            Side::Aux => &*imp.aux_test_tone_button,
            Side::Main => &*imp.main_test_tone_button,
        };
        btn.set_sensitive(false);

        // re-enable once the sweep finishes
        let btn_send = glib::SendWeakRef::from(btn.downgrade());
        let on_done = move || {
            if let Some(b) = btn_send.upgrade() {
                b.set_sensitive(true);
            }
        };

        if side == Side::Main && imp.surround_active.get() {
            backend.play_surround_test_tone(on_done);
        } else {
            backend.play_test_tone(side, on_done);
        }
    }

    fn update_readout_labels(&self) {
        let imp = self.imp();
        let (aux, main) = mixer::calculate_multipliers(imp.mix_scale.value());
        let mode = imp.volume_display.get();

        for (mul, muted, val, unit, vbox) in [
            (
                aux,
                imp.aux_mute_button.is_active(),
                &*imp.aux_volume_value,
                &*imp.aux_volume_unit,
                &*imp.aux_volume_box,
            ),
            (
                main,
                imp.main_mute_button.is_active(),
                &*imp.main_volume_value,
                &*imp.main_volume_unit,
                &*imp.main_volume_box,
            ),
        ] {
            if muted {
                val.set_text("Muted");
                unit.set_text("");
                vbox.remove_css_class("attenuated");
                vbox.add_css_class("muted");
            } else {
                let (n, u) = mode.format_parts(mul);
                val.set_text(&n);
                unit.set_text(u);
                vbox.remove_css_class("muted");
                if mul < 1.0 - f64::EPSILON {
                    vbox.add_css_class("attenuated");
                } else {
                    vbox.remove_css_class("attenuated");
                }
            }
        }
    }

    fn refresh_default_banner(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        // disables Main's default banner/tag when hw is disconnected
        if imp.main_disconnected.get() {
            imp.main_default_banner.set_visible(false);
            imp.main_default_tag.set_visible(false);
            return;
        }

        let is_default = backend.is_default(self.active_main_sink());
        // only when the active virtual output isn't default; the tag confirms when it is
        imp.main_default_banner
            .set_visible(is_default == Some(false));
        imp.main_default_tag.set_visible(is_default == Some(true));
    }

    fn on_hw_selected(&self, side: Side) {
        let imp = self.imp();
        if imp.suppress_selected.get() {
            return;
        }

        let dropdown = match side {
            Side::Aux => &*imp.aux_hw_dropdown,
            Side::Main => &*imp.main_hw_dropdown,
        };
        let Some(sink) = selected_hw_sink(dropdown) else {
            return;
        };
        // node_id 0 is the disconnected placeholder, not a real output device
        if sink.node_id == 0 {
            return;
        }
        let hw_name = sink.name.clone();

        let mut cfg = config::load();
        *cfg.side_mut(side) = sink.into();
        config::store(&cfg);
        pw_config::write_config(&cfg);

        // route live now; the new conf write is the default for next session
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };
        backend.retarget(side, &hw_name);

        // picking a new output device while in hw disonnected state rebuilds the side
        let was_disc = match side {
            Side::Aux => imp.aux_disconnected.get(),
            Side::Main => imp.main_disconnected.get(),
        };

        if was_disc {
            let sinks = backend.hw_sinks();
            imp.suppress_selected.set(true);
            self.refresh_side_dropdown(side, &sinks, &cfg);
            imp.suppress_selected.set(false);
            self.sync_controls();
        }

        self.refresh_channels_label(side);
    }

    fn refresh_channels_label(&self, side: Side) {
        let imp = self.imp();
        let (dropdown, label, row, disc) = match side {
            Side::Aux => (
                &*imp.aux_hw_dropdown,
                &*imp.aux_channels_label,
                &*imp.aux_status_row,
                imp.aux_disconnected.get(),
            ),
            Side::Main => (
                &*imp.main_hw_dropdown,
                &*imp.main_channels_label,
                &*imp.main_status_row,
                imp.main_disconnected.get(),
            ),
        };

        // remove the row when the hw is disconnected; no status to display
        if disc {
            row.set_visible(false);
            return;
        }
        row.set_visible(true);

        let text = selected_hw_sink(dropdown)
            .map(|s| {
                let mut text = crate::audio::hw_sink::channel_layout_label(s.channels, &s.position);
                if let Some(conn) = s.connection_label() {
                    if text.is_empty() {
                        text = conn.to_owned();
                    } else {
                        text = format!("{text} · {conn}");
                    }
                }
                text
            })
            .unwrap_or_default();
        label.set_text(&text);
    }

    // Aux card stream-count button
    fn refresh_aux_stream_button(&self) {
        let imp = self.imp();
        let count = imp
            .backend
            .borrow()
            .as_ref()
            .map_or(0, |b| b.aux_stream_count());
        imp.aux_streams_label.set_label(&stream_count(count));
        imp.aux_streams_button.set_sensitive(count > 0);
        if count > 0 {
            imp.aux_streams_button.remove_css_class("flat");
        } else {
            imp.aux_streams_button.add_css_class("flat");
        }
        if imp
            .aux_streams_button
            .popover()
            .is_some_and(|p| p.is_visible())
        {
            self.populate_aux_streams_list();
        }
    }

    fn populate_aux_streams_list(&self) {
        let imp = self.imp();
        let Some(list) = imp.aux_streams_list.borrow().clone() else {
            return;
        };
        let streams = imp
            .backend
            .borrow()
            .as_ref()
            .map(|b| b.aux_streams())
            .unwrap_or_default();
        stream_list::fill_streams(&list, &streams);
    }
}

fn add_css() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_resource("/io/github/arulan/Dashboard/style.css");
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
