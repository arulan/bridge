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

use std::cell::{Cell, OnceCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use glib::subclass::InitializingObject;
use gtk4::{self as gtk, CompositeTemplate};

use crate::audio::backend::PipeWireBackend;
use crate::audio::hw_sink::{channel_layout_label, disconnected_label};
use crate::audio::{mixer, pw_config};
use crate::config::{self, Side};
use crate::util::style_level_meter;
use crate::volume::VolumeDisplay;

// The detail/config page for our virtual device
#[derive(CompositeTemplate, Default)]
#[template(resource = "/io/github/arulan/Bridge/ui/sink-page.ui")]
pub struct SinkPageImp {
    #[template_child]
    pub page_title: TemplateChild<adw::WindowTitle>,
    #[template_child]
    pub disconnect_banner: TemplateChild<adw::Banner>,
    #[template_child]
    pub trim_title: TemplateChild<gtk::Label>,
    #[template_child]
    pub trim_scale: TemplateChild<gtk::Scale>,
    #[template_child]
    pub trim_value: TemplateChild<gtk::Label>,
    #[template_child]
    pub trim_unit: TemplateChild<gtk::Label>,
    #[template_child]
    pub trim_meter: TemplateChild<gtk::LevelBar>,
    #[template_child]
    pub device_value: TemplateChild<gtk::Label>,
    #[template_child]
    pub layout_row: TemplateChild<adw::ActionRow>,
    #[template_child]
    pub layout_value: TemplateChild<gtk::Label>,
    #[template_child]
    pub connection_row: TemplateChild<adw::ActionRow>,
    #[template_child]
    pub connection_value: TemplateChild<gtk::Label>,

    side: OnceCell<Side>,
    // Separate trim when Main is in Surround mode
    surround: Cell<bool>,
    suppress_trim: Cell<bool>,
    volume_display: Cell<VolumeDisplay>,
    settings: OnceCell<gio::Settings>,
}

#[glib::object_subclass]
impl ObjectSubclass for SinkPageImp {
    const NAME: &'static str = "BridgeSinkPage";
    type Type = SinkPage;
    type ParentType = adw::NavigationPage;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for SinkPageImp {
    fn signals() -> &'static [glib::subclass::Signal] {
        use std::sync::OnceLock;

        static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| vec![glib::subclass::Signal::builder("trim-changed").build()])
    }
}

impl WidgetImpl for SinkPageImp {}
impl NavigationPageImpl for SinkPageImp {}

glib::wrapper! {
    pub struct SinkPage(ObjectSubclass<SinkPageImp>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SinkPage {
    pub fn new(side: Side, backend: &PipeWireBackend) -> Self {
        let obj: Self = glib::Object::new();
        let imp = obj.imp();

        let _ = imp.side.set(side);
        let name = match side {
            Side::Aux => "Aux",
            Side::Main => "Main",
        };
        obj.set_title(name);
        imp.page_title.set_title(name);
        imp.trim_title.set_label(&format!("{name} level"));

        imp.volume_display.set(VolumeDisplay::load());
        style_level_meter(&imp.trim_meter);

        obj.refresh(backend);

        imp.trim_scale.connect_value_changed(glib::clone!(
            #[weak(rename_to = page)]
            obj,
            move |scale| page.on_trim_changed(scale.value())
        ));

        let settings = crate::application::settings();
        settings.connect_changed(
            Some("volume-display"),
            glib::clone!(
                #[weak(rename_to = page)]
                obj,
                move |_, _| {
                    page.imp().volume_display.set(VolumeDisplay::load());
                    page.update_readout();
                }
            ),
        );
        let _ = imp.settings.set(settings);

        obj
    }

    pub fn set_peak(&self, value: f64) {
        self.imp().trim_meter.set_value(value);
    }

    pub fn side(&self) -> Side {
        *self
            .imp()
            .side
            .get()
            .expect("SinkPage built outside SinkPage::new")
    }

    pub fn refresh(&self, backend: &PipeWireBackend) {
        let imp = self.imp();
        let surround = self.side() == Side::Main && config::surround_active();
        imp.surround.set(surround);

        imp.page_title
            .set_subtitle(pw_config::sink_description(self.side(), surround));

        let target = mixer::multiplier_to_trim(self.stored_trim());

        if (imp.trim_scale.value() - target).abs() > 1e-9 {
            imp.suppress_trim.set(true);
            imp.trim_scale.set_value(target);
            imp.suppress_trim.set(false);
        }

        self.update_readout();
        self.refresh_output(backend);
    }

    fn stored_trim(&self) -> f64 {
        if self.imp().surround.get() {
            config::surround_trim()
        } else {
            config::trim(self.side())
        }
    }

    fn on_trim_changed(&self, position: f64) {
        let imp = self.imp();
        if imp.suppress_trim.get() {
            return;
        }

        let mul = mixer::trim_to_multiplier(position);
        self.update_readout();

        if imp.surround.get() {
            config::set_surround_trim(mul);
        } else {
            config::set_trim(self.side(), mul);
        }

        self.emit_by_name::<()>("trim-changed", &[]);
    }

    fn update_readout(&self) {
        let imp = self.imp();
        let mul = mixer::trim_to_multiplier(imp.trim_scale.value());
        let (value, unit) = imp.volume_display.get().format_parts(mul);
        imp.trim_value.set_text(&value);
        imp.trim_unit.set_text(unit);
    }

    fn refresh_output(&self, backend: &PipeWireBackend) {
        let imp = self.imp();
        let cfg = config::load();

        let surround_def;
        let def = if imp.surround.get() {
            let sc = config::load_surround();
            surround_def = config::SinkDef {
                channels: 2,
                position: "FL,FR".to_owned(),
                hw_name: sc.hw_name,
                display_name: sc.display_name,
            };
            &surround_def
        } else {
            cfg.side(self.side())
        };

        if def.hw_name.is_empty() {
            imp.device_value.set_label("Not configured");
            imp.layout_row.set_visible(false);
            imp.connection_row.set_visible(false);
            imp.disconnect_banner.set_revealed(false);
            return;
        }

        let live = backend
            .hw_sinks()
            .into_iter()
            .find(|sink| sink.name == def.hw_name);

        let Some(sink) = live else {
            imp.device_value
                .set_label(&disconnected_label(&def.display_name));
            imp.layout_value
                .set_label(&channel_layout_label(def.channels, &def.position));
            imp.layout_row.set_visible(true);
            imp.connection_row.set_visible(false);
            imp.disconnect_banner.set_revealed(true);
            return;
        };

        imp.disconnect_banner.set_revealed(false);
        imp.device_value.set_label(&sink.display_name);
        imp.layout_value
            .set_label(&channel_layout_label(sink.channels, &sink.position));
        imp.layout_row.set_visible(true);

        match sink.connection_label() {
            Some(conn) => {
                imp.connection_value.set_label(conn);
                imp.connection_row.set_visible(true);
            }
            None => imp.connection_row.set_visible(false),
        }
    }

    pub fn connect_trim_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("trim-changed", false, move |args| {
            let page = args[0].get::<Self>().unwrap();
            f(&page);
            None
        })
    }
}
