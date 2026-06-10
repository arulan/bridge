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

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk4::{self as gtk};

use crate::audio::hw_sink::HwSink;
use crate::audio::pw_config;
use crate::config::{Side, SinkConfig};
use crate::util::ellipsize_string_factory;

#[derive(Default)]
pub struct SetupDialogImp {
    hw_sinks:        RefCell<Vec<HwSink>>,
    aux_dropdown:    RefCell<Option<gtk::DropDown>>,
    main_dropdown:   RefCell<Option<gtk::DropDown>>,
    files_container: RefCell<Option<gtk::Box>>,
    responded:       Cell<bool>,
}

#[glib::object_subclass]
impl ObjectSubclass for SetupDialogImp {
    const NAME: &'static str = "DashboardSetupDialog";
    type Type = SetupDialog;
    type ParentType = adw::Window;
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
impl WindowImpl for SetupDialogImp {
    fn close_request(&self) -> glib::Propagation {
        if !self.responded.get() {
            self.obj().respond(false);
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    }
}
impl AdwWindowImpl for SetupDialogImp {}

glib::wrapper! {
    pub struct SetupDialog(ObjectSubclass<SetupDialogImp>)
        @extends adw::Window, gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
                    gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl SetupDialog {
    pub fn new(
        hw_sinks: Vec<HwSink>,
        aux_default_id: Option<u32>,
        main_default_id: Option<u32>,
        transient_for: Option<&impl IsA<gtk::Window>>,
    ) -> Self {
        let obj: Self = glib::Object::builder()
            .property("title", "Set Up Dashboard")
            .property("default-width", 520i32)
            .property("modal", true)
            .property("resizable", true)
            .build();


        if let Some(parent) = transient_for {
            obj.set_transient_for(Some(parent));
        }

        *obj.imp().hw_sinks.borrow_mut() = hw_sinks.clone();
        obj.build_ui(&hw_sinks, aux_default_id, main_default_id);
        obj
    }

    /// The selected sink layout
    pub fn sink_config(&self) -> SinkConfig {
        SinkConfig {
            aux:  self.selected_sink(Side::Aux).into(),
            main: self.selected_sink(Side::Main).into(),
        }
    }

    fn selected_sink(&self, side: Side) -> HwSink {
        let imp = self.imp();
        let dropdown = match side {
            Side::Aux  => imp.aux_dropdown.borrow(),
            Side::Main => imp.main_dropdown.borrow(),
        };
        let idx = dropdown.as_ref().map(|d| d.selected()).unwrap_or(0) as usize;
        imp.hw_sinks.borrow()[idx].clone()
    }


    fn respond(&self, approved: bool) {
        let imp = self.imp();
        if imp.responded.get() { return; }
        imp.responded.set(true);
        self.close();
        self.emit_by_name::<()>(if approved { "approved" } else { "declined" }, &[]);
    }

    fn on_device_changed(&self) {
        self.rebuild_files();
    }

    fn rebuild_files(&self) {
        let imp = self.imp();
        let Some(container) = imp.files_container.borrow().clone() else { return };
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        if imp.hw_sinks.borrow().is_empty() { return; }
        for (path, content) in pw_config::preview_files(&self.sink_config()) {
            container.append(&make_file_row(&path, &content));
        }
    }

    fn build_ui(
        &self,
        hw_sinks: &[HwSink],
        aux_default_id: Option<u32>,
        main_default_id: Option<u32>,
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
            .margin_top(24).margin_bottom(24)
            .margin_start(20).margin_end(20)
            .build();

        let desc = gtk::Label::new(None);
        desc.set_markup(
            "Dashboard creates two virtual outputs — \
             <b>Aux</b> and <b>Main</b> — that you can mix independently. \
             Each mirrors the channel layout of the configured output device.\n\n\
             The written PipeWire configuration allows them to persist after your next login."
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
            let labels: Vec<&str> = hw_sinks.iter().map(|s| s.display_name.as_str()).collect();
            let model = gtk::StringList::new(&labels);

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
            aux_dd.set_factory(Some(&ellipsize_string_factory()));
    
            let main_dd = gtk::DropDown::builder()
                .model(&model)
                .selected(main_idx)
                .hexpand(true)
                .build();
            main_dd.set_factory(Some(&ellipsize_string_factory()));

            body.append(&make_device_row("Aux output", &aux_dd));
            body.append(&make_device_row("Main output", &main_dd));

            let obj_c = self.clone();
            aux_dd.connect_selected_notify(move |_| obj_c.on_device_changed());
            let obj_c = self.clone();
            main_dd.connect_selected_notify(move |_| obj_c.on_device_changed());

            *imp.aux_dropdown.borrow_mut()  = Some(aux_dd);
            *imp.main_dropdown.borrow_mut() = Some(main_dd);
        } else {
            let warn = gtk::Label::new(Some("No audio output devices found"));
            warn.set_xalign(0.0);
            warn.add_css_class("error");
            body.append(&warn);
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
        self.set_content(Some(&toolbar));
    }
}

fn make_device_row(label_text: &str, dropdown: &gtk::DropDown) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .valign(gtk::Align::Center)
        .build();
    let lbl = gtk::Label::builder()
        .label(label_text)
        .xalign(0.0)
        .hexpand(true)
        .build();
    row.append(&lbl);
    row.append(dropdown);
    row
}

fn make_file_row(path: &str, content: &str) -> gtk::Box {
    let home = std::env::var("HOME").unwrap_or_default();
    let display_path = path.replacen(&home, "~", 1);

    // TODO: Check with GNOME HIG on EllipsizeMode recommendation
    let lbl = gtk::Label::builder()
        .label(&display_path)
        .xalign(0.0)
        .hexpand(true)
        .max_width_chars(1)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .tooltip_text(&display_path)
        .build();
    lbl.add_css_class("monospace");
    lbl.add_css_class("caption");

    let expander = gtk::Expander::new(None);
    expander.set_label_widget(Some(&lbl));

    let tv = gtk::TextView::builder()
        .editable(false)
        .monospace(true)
        .cursor_visible(false)
        .top_margin(10).bottom_margin(10)
        .left_margin(12).right_margin(12)
        .build();
    tv.buffer().set_text(content.trim());

    let sw = gtk::ScrolledWindow::builder()
        .min_content_height(180)
        .max_content_height(300)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&tv)
        .build();

    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&sw));
    expander.set_child(Some(&frame));

    let boxw = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    boxw.append(&expander);
    boxw
}
