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

use std::cell::{Cell, OnceCell, RefCell};

use adw::subclass::prelude::*;
use gtk4::{self as gtk, prelude::*, CompositeTemplate};
use glib::subclass::InitializingObject;

use crate::audio::backend::PipeWireBackend;
use crate::audio::{mixer, pw_config};
use crate::config::{self, Side};
use crate::util::{hw_sink_factory, hw_sink_model, selected_hw_sink};
use crate::volume::VolumeDisplay;

#[derive(CompositeTemplate, Default)]
#[template(file = "../data/ui/window.ui")]
pub struct DashboardWindowImp {
    #[template_child] pub persist_banner: TemplateChild<adw::Banner>,
    #[template_child] pub aux_hw_dropdown:  TemplateChild<gtk::DropDown>,
    #[template_child] pub main_hw_dropdown: TemplateChild<gtk::DropDown>,
    #[template_child] pub aux_mute_button:  TemplateChild<gtk::ToggleButton>,
    #[template_child] pub main_mute_button: TemplateChild<gtk::ToggleButton>,
    #[template_child] pub aux_mute_image:   TemplateChild<gtk::Image>,
    #[template_child] pub main_mute_image:  TemplateChild<gtk::Image>,
    #[template_child] pub aux_test_tone_button:  TemplateChild<gtk::Button>,
    #[template_child] pub main_test_tone_button: TemplateChild<gtk::Button>,
    #[template_child] pub mix_scale: TemplateChild<gtk::Scale>,
    #[template_child] pub aux_side_label:  TemplateChild<gtk::Label>,
    #[template_child] pub main_side_label: TemplateChild<gtk::Label>,
    #[template_child] pub main_default_banner: TemplateChild<gtk::Box>,
    #[template_child] pub main_default_button: TemplateChild<gtk::Button>,

    backend:        RefCell<Option<PipeWireBackend>>,
    suppress_selected: Cell<bool>,

    volume_display: Cell<VolumeDisplay>,
    settings:       OnceCell<gio::Settings>,
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
        glib::Object::builder()
            .property("application", app)
            .build()
    }

    pub fn setup(&self, backend: &PipeWireBackend) {
        let imp = self.imp();

        // TODO: Look into GResource later
        add_css();

        imp.aux_hw_dropdown.set_factory(Some(&hw_sink_factory()));
        imp.main_hw_dropdown.set_factory(Some(&hw_sink_factory()));

        imp.aux_hw_dropdown.connect_selected_notify(glib::clone!(
            #[weak(rename_to = w)] self,
            move |_| w.on_hw_selected(Side::Aux)
        ));
        imp.main_hw_dropdown.connect_selected_notify(glib::clone!(
            #[weak(rename_to = w)] self,
            move |_| w.on_hw_selected(Side::Main)
        ));

        imp.persist_banner.connect_button_clicked(glib::clone!(
            #[weak(rename_to = w)] self,
            move |_| w.imp().persist_banner.set_revealed(false)
        ));

        imp.mix_scale.connect_value_changed(glib::clone!(
            #[weak(rename_to = w)] self,
            move |_| w.apply_mix()
        ));

        imp.aux_mute_button.connect_toggled(glib::clone!(
            #[weak(rename_to = w)] self,
            move |b| w.on_mute_toggled(Side::Aux, b.is_active())
        ));
        imp.main_mute_button.connect_toggled(glib::clone!(
            #[weak(rename_to = w)] self,
            move |b| w.on_mute_toggled(Side::Main, b.is_active())
        ));

        imp.aux_test_tone_button.connect_clicked(glib::clone!(
            #[weak(rename_to = w)] self,
            move |_| w.on_test_clicked(Side::Aux)
        ));
        imp.main_test_tone_button.connect_clicked(glib::clone!(
            #[weak(rename_to = w)] self,
            move |_| w.on_test_clicked(Side::Main)
        ));

        imp.volume_display.set(VolumeDisplay::load());
        let settings = crate::application::settings();
        settings.connect_changed(Some("volume-display"), glib::clone!(
            #[weak(rename_to = w)] self,
            move |_, _| {
                w.imp().volume_display.set(VolumeDisplay::load());
                w.update_readout_labels();
            }
        ));
        let _ = imp.settings.set(settings);
        self.update_readout_labels();

        imp.main_default_button.connect_clicked(glib::clone!(
            #[weak(rename_to = w)] self,
            move |_| {
                let Some(backend) = w.imp().backend.borrow().clone() else { return };
                backend.set_main_default();
            }
        ));

        backend.connect_sinks_ready(glib::clone!(
            #[weak(rename_to = w)] self,
            move |_| w.populate_dropdowns()
        ));

        backend.connect_default_changed(glib::clone!(
            #[weak(rename_to = w)] self,
            move |_| w.refresh_default_banner()
        ));
        *imp.backend.borrow_mut() = Some(backend.clone());
    }

    pub fn populate_dropdowns(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else { return };

        let sinks = backend.hw_sinks();
        let cfg = config::load();
        let model = hw_sink_model(&sinks);

        // guard against set_model & set_selected firing notify::selected 
        // when user hasn't changed hw_dropdown
        imp.suppress_selected.set(true);
        for (dropdown, hw_name) in [
            (&*imp.aux_hw_dropdown,  &cfg.aux.hw_name),
            (&*imp.main_hw_dropdown, &cfg.main.hw_name),
        ] {
            dropdown.set_model(Some(&model));
            let idx = sinks.iter().position(|s| &s.name == hw_name).unwrap_or(0) as u32;
            dropdown.set_selected(idx);
        }
        imp.suppress_selected.set(false);

        // crossfader and mute are disabled until our virtual sinks exist
        let present = backend.owned_sinks_present();
        imp.mix_scale.set_sensitive(present);
        imp.aux_mute_button.set_sensitive(present);
        imp.main_mute_button.set_sensitive(present);
        imp.aux_test_tone_button.set_sensitive(present);
        imp.main_test_tone_button.set_sensitive(present);

        if present {
            self.apply_mix();
            backend.set_mute(Side::Aux, imp.aux_mute_button.is_active());
            backend.set_mute(Side::Main, imp.main_mute_button.is_active());
        }

        // only display when persistent virtual sinks aren't live yet
        imp.persist_banner.set_revealed(!present);
    }

    pub fn reveal_persist_banner(&self) {
        self.imp().persist_banner.set_revealed(true);
    }

    fn apply_mix(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else { return };

        let (aux, main) = mixer::calculate_multipliers(imp.mix_scale.value());

        backend.set_volume(Side::Aux, aux);
        backend.set_volume(Side::Main, main);

        self.update_readout_labels();
    }

    fn on_mute_toggled(&self, side: Side, muted: bool) {
        let imp = self.imp();

        let (img, btn) = match side {
            Side::Aux  => (&*imp.aux_mute_image,  &*imp.aux_mute_button),
            Side::Main => (&*imp.main_mute_image, &*imp.main_mute_button),
        };
        img.set_icon_name(Some(if muted { "audio-volume-muted-symbolic" } else { "audio-volume-high-symbolic" }));
        btn.set_tooltip_text(Some(if muted { "Unmute this output" } else { "Mute this output" }));

        if let Some(backend) = imp.backend.borrow().clone() {
            backend.set_mute(side, muted);
        }
    }

    fn on_test_clicked(&self, side: Side) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else { return };

        let btn = match side {
            Side::Aux  => &*imp.aux_test_tone_button,
            Side::Main => &*imp.main_test_tone_button,
        };
        btn.set_sensitive(false);

        // re-enable once the sweep finishes
        let btn_send = glib::SendWeakRef::from(btn.downgrade());
        backend.play_test_tone(side, move || {
            if let Some(b) = btn_send.upgrade() {
                b.set_sensitive(true);
            }
        });
    }

    fn update_readout_labels(&self) {
        let imp = self.imp();
        let (aux, main) = mixer::calculate_multipliers(imp.mix_scale.value());
        let mode = imp.volume_display.get();
        imp.aux_side_label.set_text(&format!("Aux {}", mode.format(aux)));
        imp.main_side_label.set_text(&format!("{} Main", mode.format(main)));
    }

    fn refresh_default_banner(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else { return };

        // only when Main isn't default sink
        imp.main_default_banner.set_visible(backend.main_is_default() == Some(false));
    }

    fn on_hw_selected(&self, side: Side) {
        let imp = self.imp();
        if imp.suppress_selected.get() { return }

        let dropdown = match side {
            Side::Aux  => &*imp.aux_hw_dropdown,
            Side::Main => &*imp.main_hw_dropdown,
        };
        let Some(sink) = selected_hw_sink(dropdown) else { return };

        let mut cfg = config::load();
        *cfg.side_mut(side) = sink.into();
        config::store(&cfg);
        pw_config::write_config(&cfg);

        self.reveal_persist_banner();
    }
}

fn add_css() {
    let Some(display) = gtk::gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("../data/style.css"));
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
