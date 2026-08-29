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

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk4::{self as gtk};

use crate::audio::hw_device::HwDevice;
use crate::audio::pw_config;
use crate::config::{self, Side, SinkConfig, SinkDef};
use crate::util::{
    disconnected_device, hw_device_factory, hw_device_model, make_device_row, make_file_row,
    selected_hw_device,
};

pub enum MicPreselect {
    /// node_id
    Device(u32),
    /// Configured, but disconnected
    Disconnected(SinkDef),
    /// Nothing configured
    None,
}

#[derive(Default)]
pub struct SetupDialogImp {
    aux_dropdown: RefCell<Option<gtk::DropDown>>,
    main_dropdown: RefCell<Option<gtk::DropDown>>,
    mic_dropdown: RefCell<Option<gtk::DropDown>>,
    files_container: RefCell<Option<gtk::Box>>,
    responded: Cell<bool>,
}

#[glib::object_subclass]
impl ObjectSubclass for SetupDialogImp {
    const NAME: &'static str = "BridgeSetupDialog";
    type Type = SetupDialog;
    type ParentType = adw::Dialog;
}

impl ObjectImpl for SetupDialogImp {
    fn signals() -> &'static [glib::subclass::Signal] {
        use std::sync::OnceLock;

        static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| {
            vec![
                glib::subclass::Signal::builder("approved").build(),
                glib::subclass::Signal::builder("declined").build(),
            ]
        })
    }
}

impl WidgetImpl for SetupDialogImp {}
impl AdwDialogImpl for SetupDialogImp {
    fn closed(&self) {
        self.obj().respond(false);
    }
}

glib::wrapper! {
    pub struct SetupDialog(ObjectSubclass<SetupDialogImp>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
                    gtk::ShortcutManager;
}

impl SetupDialog {
    pub fn new(
        hw_sinks: Vec<HwDevice>,
        hw_sources: Vec<HwDevice>,
        aux_default_id: Option<u32>,
        main_default_id: Option<u32>,
        mic_preselect: MicPreselect,
    ) -> Self {
        let obj: Self = glib::Object::builder()
            .property("title", "Set Up Bridge")
            .property("content-width", 520i32)
            .build();

        obj.build_ui(
            &hw_sinks,
            &hw_sources,
            aux_default_id,
            main_default_id,
            mic_preselect,
        );
        obj
    }

    /// The selected sink layout
    pub fn sink_config(&self) -> SinkConfig {
        SinkConfig {
            aux: self.selected_sink(Side::Aux).into(),
            main: self.selected_sink(Side::Main).into(),
        }
    }

    /// The selected mic input, or None for the opt-out option
    pub fn mic_def(&self) -> Option<SinkDef> {
        let dropdown = self.imp().mic_dropdown.borrow();
        let device = dropdown.as_ref().and_then(selected_hw_device)?;
        // Only the opt-out selection has no node.name
        if device.name.is_empty() {
            return None;
        }
        // node_id 0 is the disconnected placeholder
        if device.node_id == 0 {
            return Some(config::load_mic());
        }
        Some(device.into())
    }

    /// False when mic_def is False due to no source devices being available
    pub fn mic_offered(&self) -> bool {
        self.imp().mic_dropdown.borrow().is_some()
    }

    fn selected_sink(&self, side: Side) -> HwDevice {
        let imp = self.imp();
        let dropdown = match side {
            Side::Aux => imp.aux_dropdown.borrow(),
            Side::Main => imp.main_dropdown.borrow(),
        };
        dropdown
            .as_ref()
            .and_then(selected_hw_device)
            .expect("selected_sink called with no device selected")
    }

    fn respond(&self, approved: bool) {
        let imp = self.imp();
        if imp.responded.get() {
            return;
        }
        imp.responded.set(true);
        self.close();
        self.emit_by_name::<()>(if approved { "approved" } else { "declined" }, &[]);
    }

    fn on_device_changed(&self) {
        self.rebuild_files();
    }

    fn rebuild_files(&self) {
        let imp = self.imp();
        let Some(container) = imp.files_container.borrow().clone() else {
            return;
        };
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        if imp.aux_dropdown.borrow().is_some() {
            for (path, content) in pw_config::preview_files(&self.sink_config()) {
                container.append(&make_file_row(&path, &content));
            }
        }
        if let Some(def) = self.mic_def() {
            for (path, content) in pw_config::mic_preview_files(&def) {
                container.append(&make_file_row(&path, &content));
            }
        }
    }

    fn build_ui(
        &self,
        hw_sinks: &[HwDevice],
        hw_sources: &[HwDevice],
        aux_default_id: Option<u32>,
        main_default_id: Option<u32>,
        mic_preselect: MicPreselect,
    ) {
        let imp = self.imp();

        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        toolbar.add_top_bar(&header);

        // Following GNOME HIG on dialog button placement

        // Cancel
        let cancel_btn = gtk::Button::with_label("Cancel");
        let obj_cancel = self.clone();
        cancel_btn.connect_clicked(move |_| obj_cancel.respond(false));
        header.pack_start(&cancel_btn);

        // Set Up
        let setup_btn = gtk::Button::with_label("Set Up");
        setup_btn.add_css_class("suggested-action");
        setup_btn.set_sensitive(!hw_sinks.is_empty());
        let obj_setup = self.clone();
        setup_btn.connect_clicked(move |_| obj_setup.respond(true));
        header.pack_end(&setup_btn);
        self.set_default_widget(Some(&setup_btn));

        let outer_scroll = gtk::ScrolledWindow::builder()
            .propagate_natural_height(true)
            .max_content_height(680)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let clamp = adw::Clamp::builder()
            .maximum_size(500)
            .tightening_threshold(400)
            .build();

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(16)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(20)
            .margin_end(20)
            .build();

        let desc = gtk::Label::new(None);
        desc.set_markup(
            "Bridge creates two virtual outputs — \
             <b>Aux</b> and <b>Main</b> — that you can mix independently. \
             Each mirrors the channel layout of the configured output device.\n\n\
             You must login again to persist the outputs beyond the current session.",
        );
        desc.set_wrap(true);
        desc.set_xalign(0.0);
        body.append(&desc);
        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let devices_heading = gtk::Label::new(Some("Output device for virtual output"));
        devices_heading.set_xalign(0.0);
        devices_heading.add_css_class("heading");
        body.append(&devices_heading);

        if !hw_sinks.is_empty() {
            let model = hw_device_model(hw_sinks);

            let aux_idx = aux_default_id
                .and_then(|id| hw_sinks.iter().position(|s| s.node_id == id))
                .unwrap_or(0) as u32;

            let main_idx = main_default_id
                .and_then(|id| hw_sinks.iter().position(|s| s.node_id == id))
                .unwrap_or(0) as u32;

            let aux_dd = gtk::DropDown::builder()
                .model(&model)
                .selected(aux_idx)
                .hexpand(true)
                .build();
            aux_dd.set_factory(Some(&hw_device_factory()));

            let main_dd = gtk::DropDown::builder()
                .model(&model)
                .selected(main_idx)
                .hexpand(true)
                .build();
            main_dd.set_factory(Some(&hw_device_factory()));

            body.append(&make_device_row("Aux output", &aux_dd));
            body.append(&make_device_row("Main output", &main_dd));

            let obj_c = self.clone();
            aux_dd.connect_selected_notify(move |_| obj_c.on_device_changed());
            let obj_c = self.clone();
            main_dd.connect_selected_notify(move |_| obj_c.on_device_changed());

            *imp.aux_dropdown.borrow_mut() = Some(aux_dd);
            *imp.main_dropdown.borrow_mut() = Some(main_dd);
        } else {
            let warn = gtk::Label::new(Some("No audio output devices found"));
            warn.set_xalign(0.0);
            warn.add_css_class("error");
            body.append(&warn);
        }

        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let mic_heading = gtk::Label::new(Some("Microphone (optional)"));
        mic_heading.set_xalign(0.0);
        mic_heading.add_css_class("heading");
        body.append(&mic_heading);

        let mic_desc = gtk::Label::new(Some(
            "Bridge can create a virtual microphone, \
             fed by the input device you pick",
        ));
        mic_desc.set_wrap(true);
        mic_desc.set_xalign(0.0);
        mic_desc.add_css_class("dim-label");
        body.append(&mic_desc);

        let disconnected = matches!(mic_preselect, MicPreselect::Disconnected(_));
        if hw_sources.is_empty() && !disconnected {
            let none = gtk::Label::new(Some("No microphone found"));
            none.set_xalign(0.0);
            none.add_css_class("dim-label");
            body.append(&none);
        } else {
            let mic_dd = mic_dropdown(hw_sources, &mic_preselect);
            body.append(&make_device_row("Input device", &mic_dd));

            let obj_c = self.clone();
            mic_dd.connect_selected_notify(move |_| obj_c.on_device_changed());

            *imp.mic_dropdown.borrow_mut() = Some(mic_dd);
        }

        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let files_heading = gtk::Label::new(Some("Configuration preview"));
        files_heading.set_xalign(0.0);
        files_heading.add_css_class("heading");
        body.append(&files_heading);

        let files_container = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        body.append(&files_container);
        *imp.files_container.borrow_mut() = Some(files_container);
        self.rebuild_files();

        clamp.set_child(Some(&body));
        outer_scroll.set_child(Some(&clamp));
        toolbar.set_content(Some(&outer_scroll));
        self.set_child(Some(&toolbar));
    }
}

fn mic_dropdown(hw_sources: &[HwDevice], preselect: &MicPreselect) -> gtk::DropDown {
    let model = hw_device_model(hw_sources);
    let skip = HwDevice {
        node_id: 0,
        name: String::new(),
        display_name: "Don't set up a microphone".to_owned(),
        device_api: String::new(),
        device_bus: String::new(),
        profile_name: String::new(),
        channels: 0,
        position: String::new(),
    };
    model.insert(0, &glib::BoxedAnyObject::new(skip));

    let idx = match preselect {
        MicPreselect::Device(id) => hw_sources
            .iter()
            .position(|s| s.node_id == *id)
            .map(|i| i as u32 + 1)
            .unwrap_or(0),
        MicPreselect::Disconnected(def) => {
            model.insert(1, &glib::BoxedAnyObject::new(disconnected_device(def)));
            1
        }
        MicPreselect::None => 0,
    };

    let dropdown = gtk::DropDown::builder()
        .model(&model)
        .selected(idx)
        .hexpand(true)
        .build();
    dropdown.set_factory(Some(&hw_device_factory()));
    dropdown
}
