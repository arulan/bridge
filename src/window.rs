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

use std::cell::{Cell, RefCell};

use adw::subclass::prelude::*;
use gtk4::{self as gtk, CompositeTemplate};
use glib::subclass::InitializingObject;

use crate::audio::backend::PipeWireBackend;
use crate::audio::pw_config;
use crate::config::{self, Side};
use crate::util::{hw_sink_factory, hw_sink_model, selected_hw_sink};

#[derive(CompositeTemplate, Default)]
#[template(file = "../data/ui/window.ui")]
pub struct DashboardWindowImp {
    #[template_child] pub persist_banner: TemplateChild<adw::Banner>,
    #[template_child] pub aux_hw_dropdown:  TemplateChild<gtk::DropDown>,
    #[template_child] pub main_hw_dropdown: TemplateChild<gtk::DropDown>,

    backend:        RefCell<Option<PipeWireBackend>>,
    suppress_selected: Cell<bool>,
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

        backend.connect_sinks_ready(glib::clone!(
            #[weak(rename_to = w)] self,
            move |_| w.populate_dropdowns()
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
    }

    pub fn reveal_persist_banner(&self) {
        self.imp().persist_banner.set_revealed(true);
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
