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
use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk4::{self as gtk};

use crate::audio::hw_sink::HwSink;
use crate::audio::pw_config;
use crate::config::SurroundConfig;
use crate::util::{
    hw_sink_factory, hw_sink_model, make_device_row, make_file_row, selected_hw_sink,
};

#[derive(Default)]
pub struct SurroundDialogImp {
    device_dropdown: RefCell<Option<gtk::DropDown>>,
    hrir_source: RefCell<Option<PathBuf>>,
    hrir_value: RefCell<Option<gtk::Label>>,
    files_container: RefCell<Option<gtk::Box>>,
    setup_button: RefCell<Option<gtk::Button>>,
    responded: Cell<bool>,
}

#[glib::object_subclass]
impl ObjectSubclass for SurroundDialogImp {
    const NAME: &'static str = "BridgeSurroundDialog";
    type Type = SurroundDialog;
    type ParentType = adw::Dialog;
}

impl ObjectImpl for SurroundDialogImp {
    fn signals() -> &'static [glib::subclass::Signal] {
        use std::sync::OnceLock;

        static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| {
            vec![
                glib::subclass::Signal::builder("approved").build(),
                glib::subclass::Signal::builder("declined").build(),
                glib::subclass::Signal::builder("reset").build(),
            ]
        })
    }
}

impl WidgetImpl for SurroundDialogImp {}
impl AdwDialogImpl for SurroundDialogImp {
    fn closed(&self) {
        self.obj().respond("declined");
    }
}

glib::wrapper! {
    pub struct SurroundDialog(ObjectSubclass<SurroundDialogImp>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
                    gtk::ShortcutManager;
}

impl SurroundDialog {
    pub fn new(hw_sinks: Vec<HwSink>, current: &SurroundConfig) -> Self {
        let obj: Self = glib::Object::builder()
            .property("title", "Virtual Surround")
            .property("content-width", 520i32)
            .build();

        obj.build_ui(&hw_sinks, current);
        obj
    }

    pub fn selected_sink(&self) -> Option<HwSink> {
        self.imp()
            .device_dropdown
            .borrow()
            .as_ref()
            .and_then(selected_hw_sink)
    }

    pub fn hrir_source(&self) -> Option<PathBuf> {
        self.imp().hrir_source.borrow().clone()
    }

    fn respond(&self, signal: &str) {
        let imp = self.imp();
        if imp.responded.get() {
            return;
        }
        imp.responded.set(true);
        self.close();
        self.emit_by_name::<()>(signal, &[]);
    }

    fn confirm_reset(&self) {
        let dialog = adw::AlertDialog::new(
            Some("Reset Virtual Surround?"),
            Some(
                "This removes the Virtual Surround configuration and returns it to the unconfigured state. \
                 Your imported HRIR files are left in place.\n\nThe change takes effect after your \
                 next login.",
            ),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("reset", "Reset");
        dialog.set_response_appearance("reset", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let obj_c = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "reset" {
                obj_c.respond("reset");
            }
        });

        dialog.present(Some(self));
    }

    fn preview_hrir_path(&self) -> Option<String> {
        self.imp().hrir_source.borrow().as_ref().and_then(|src| {
            src.file_name()
                .map(|n| pw_config::hrir_dir().join(n).to_string_lossy().into_owned())
        })
    }

    fn set_hrir(&self, path: PathBuf) {
        let imp = self.imp();
        if let Some(label) = imp.hrir_value.borrow().as_ref() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            label.set_text(&name);
            label.remove_css_class("dim-label");
        }
        imp.hrir_source.replace(Some(path));
        self.rebuild_files();
        self.update_ready();
    }

    fn choose_hrir(&self) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("HRIR wav"));
        filter.add_suffix("wav");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);

        let dialog = gtk::FileDialog::builder()
            .title("Choose HRIR File")
            .filters(&filters)
            .modal(true)
            .build();

        let parent = self.root().and_downcast::<gtk::Window>();
        dialog.open(
            parent.as_ref(),
            gio::Cancellable::NONE,
            glib::clone!(
                #[weak(rename_to = d)]
                self,
                move |res| {
                    if let Ok(file) = res
                        && let Some(path) = file.path()
                    {
                        d.set_hrir(path);
                    }
                }
            ),
        );
    }

    fn update_ready(&self) {
        let imp = self.imp();
        let has_device = imp
            .device_dropdown
            .borrow()
            .as_ref()
            .is_some_and(|d| selected_hw_sink(d).is_some());
        let ready = has_device && imp.hrir_source.borrow().is_some();
        if let Some(btn) = imp.setup_button.borrow().as_ref() {
            btn.set_sensitive(ready);
        }
    }

    fn rebuild_files(&self) {
        let imp = self.imp();
        let Some(container) = imp.files_container.borrow().clone() else {
            return;
        };
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }

        let (Some(hrir), Some(sink)) = (self.preview_hrir_path(), self.selected_sink()) else {
            let hint = gtk::Label::builder()
                .label("Choose an HRIR file to preview the configuration")
                .xalign(0.0)
                .wrap(true)
                .build();
            hint.add_css_class("caption");
            hint.add_css_class("dim-label");
            container.append(&hint);
            return;
        };
        for (path, content) in pw_config::surround_preview_files(&hrir, &sink.name) {
            container.append(&make_file_row(&path, &content));
        }
    }

    fn build_ui(&self, hw_sinks: &[HwSink], current: &SurroundConfig) {
        let imp = self.imp();
        let configured = !current.hrir_path.is_empty();

        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        toolbar.add_top_bar(&header);

        let cancel_btn = gtk::Button::with_label("Cancel");
        let obj_c = self.clone();
        cancel_btn.connect_clicked(move |_| obj_c.respond("declined"));
        header.pack_start(&cancel_btn);

        if configured {
            let reset_btn = gtk::Button::with_label("Reset");
            reset_btn.add_css_class("destructive-action");
            let obj_c = self.clone();
            reset_btn.connect_clicked(move |_| obj_c.confirm_reset());
            header.pack_start(&reset_btn);
        }

        let setup_btn = gtk::Button::with_label("Set Up");
        setup_btn.add_css_class("suggested-action");
        setup_btn.set_sensitive(false);
        let obj_c = self.clone();
        setup_btn.connect_clicked(move |_| obj_c.respond("approved"));
        header.pack_end(&setup_btn);
        self.set_default_widget(Some(&setup_btn));
        *imp.setup_button.borrow_mut() = Some(setup_btn);

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
            "Virtual Surround processes 7.1 audio through a head-related impulse response \
             (HRIR) to give positional sound on stereo <b>headphones</b>. \n\nThis creates an additional \
             virtual output — <b>Virtual Surround</b> — that can be toggled to from Main.\n\n\
             The output becomes available after your next login.",
        );
        desc.set_wrap(true);
        desc.set_xalign(0.0);
        body.append(&desc);
        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        // Output device
        let dev_heading = gtk::Label::new(Some("Headphone output"));
        dev_heading.set_xalign(0.0);
        dev_heading.add_css_class("heading");
        body.append(&dev_heading);

        if !hw_sinks.is_empty() {
            let model = hw_sink_model(hw_sinks);
            let idx = hw_sinks
                .iter()
                .position(|s| s.name == current.hw_name)
                .unwrap_or(0) as u32;

            let dropdown = gtk::DropDown::builder()
                .model(&model)
                .selected(idx)
                .hexpand(true)
                .build();
            dropdown.set_factory(Some(&hw_sink_factory()));

            let obj_c = self.clone();
            dropdown.connect_selected_notify(move |_| {
                obj_c.rebuild_files();
                obj_c.update_ready();
            });

            body.append(&make_device_row("Output device", &dropdown));
            *imp.device_dropdown.borrow_mut() = Some(dropdown);
        } else {
            let warn = gtk::Label::new(Some("No audio output devices found"));
            warn.set_xalign(0.0);
            warn.add_css_class("error");
            body.append(&warn);
        }

        // HRIR file
        let hrir_heading = gtk::Label::new(Some("HRIR file"));
        hrir_heading.set_xalign(0.0);
        hrir_heading.add_css_class("heading");
        body.append(&hrir_heading);

        let hrir_value = gtk::Label::builder()
            .label("No file chosen")
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();
        hrir_value.add_css_class("dim-label");

        let choose_btn = gtk::Button::with_label("Choose…");
        let obj_c = self.clone();
        choose_btn.connect_clicked(move |_| obj_c.choose_hrir());

        let hrir_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .valign(gtk::Align::Center)
            .build();
        hrir_row.append(&hrir_value);
        hrir_row.append(&choose_btn);
        body.append(&hrir_row);
        *imp.hrir_value.borrow_mut() = Some(hrir_value);

        let hrir_hint = gtk::Label::builder().xalign(0.0).wrap(true).build();
        hrir_hint.set_markup(
            "A 14-channel <a href=\"https://sourceforge.net/projects/hesuvi/\">HeSuVi</a> \
             .wav, or any compatible HRIR",
        );
        hrir_hint.add_css_class("caption");
        hrir_hint.add_css_class("dim-label");
        body.append(&hrir_hint);

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

        // If already configured, prefill the file
        if configured {
            self.set_hrir(PathBuf::from(&current.hrir_path));
        } else {
            self.rebuild_files();
        }

        clamp.set_child(Some(&body));
        outer_scroll.set_child(Some(&clamp));
        toolbar.set_content(Some(&outer_scroll));
        self.set_child(Some(&toolbar));
    }
}
