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

pub mod ffi;

use std::ffi::{c_void, CStr, CString};
use glib::prelude::*;
use glib::gobject_ffi::GObject;
use glib::translate::FromGlibPtrFull;
use ffi::GType;

pub use ffi::{WP_INIT_ALL, WP_PIPEWIRE_OBJECT_FEATURE_INFO, WP_PROXY_FEATURE_BOUND};

unsafe fn to_gobj_full<T>(ptr: *mut T) -> glib::Object {
    glib::Object::from_glib_full(ptr as *mut GObject)
}

pub fn init_all() {
    unsafe { ffi::wp_init(WP_INIT_ALL) }
}

/// Read node properties
pub fn node_prop(node: &glib::Object, key: &str) -> Option<String> {
    let k = CString::new(key).unwrap();
    unsafe {
        let props = ffi::wp_global_proxy_get_global_properties(
            node.as_ptr() as *mut ffi::WpGlobalProxy,
        );

        if props.is_null() {
            return None;
        }

        let val = ffi::wp_properties_get(props, k.as_ptr());

        if val.is_null() {
            None
        } else {
            Some(CStr::from_ptr(val).to_string_lossy().into_owned())
        }
    }
}

/// Reads property from the node's full PipeWire object info; fallback to global props
pub fn node_pw_prop(node: &glib::Object, key: &str) -> Option<String> {
    let k = CString::new(key).unwrap();
    unsafe {
        let val = ffi::wp_pipewire_object_get_property(
            node.as_ptr() as *mut ffi::WpPipewireObject,
            k.as_ptr(),
        );
        
        if !val.is_null() {
            return Some(CStr::from_ptr(val).to_string_lossy().into_owned());
        }
    }
    node_prop(node, key)
}

pub fn bound_id(obj: &glib::Object) -> u32 {
    unsafe { ffi::wp_proxy_get_bound_id(obj.as_ptr() as *mut ffi::WpProxy) }
}

pub fn node_type() -> GType {
    unsafe { ffi::wp_node_get_type() }
}

pub fn metadata_type() -> GType {
    unsafe { ffi::wp_metadata_get_type() }
}

pub struct Core {
    obj: glib::Object,
}

impl Core {
    pub fn new() -> Self {
        unsafe {
    

            // It looks like we can pass a NULL conf so that pw_context loads the
            // Pipewire client.conf. This pulls in the modules we need. 
            // TODO: After flatpak packaging check this assumption again
            let ptr = ffi::wp_core_new(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert!(!ptr.is_null(), "wp_core_new returned NULL");

            // No connection yet, just client build + transport
            Core { obj: to_gobj_full(ptr) }
        }
    }


    pub fn connect(&self) -> bool {
        unsafe { ffi::wp_core_connect(self.obj.as_ptr() as *mut ffi::WpCore) != 0 }
    }

    pub fn disconnect(&self) {
        unsafe { ffi::wp_core_disconnect(self.obj.as_ptr() as *mut ffi::WpCore) }
    }

    pub fn install_object_manager(&self, om: &ObjectManager) {
        unsafe {
            ffi::wp_core_install_object_manager(
                self.obj.as_ptr() as *mut ffi::WpCore,
                om.obj.as_ptr() as *mut ffi::WpObjectManager,
            )
        }
    }
}

pub struct ObjectManager {
    obj: glib::Object,
}

impl ObjectManager {
    pub fn new() -> Self {
        unsafe {
            let ptr = ffi::wp_object_manager_new();
            assert!(!ptr.is_null(), "wp_object_manager_new returned NULL");
            ObjectManager { obj: to_gobj_full(ptr) }
        }
    }

    pub fn add_interest_for_type(&self, gtype: GType) {
        unsafe {
            let interest = ffi::wp_object_interest_new_type(gtype);
            ffi::wp_object_manager_add_interest_full(
                self.obj.as_ptr() as *mut ffi::WpObjectManager,
                interest,
            );
        }
    }


    pub fn request_object_features(&self, gtype: GType, features: u32) {
        unsafe {
            ffi::wp_object_manager_request_object_features(
                self.obj.as_ptr() as *mut ffi::WpObjectManager,
                gtype,
                features,
            )
        }
    }

    pub fn connect_object_added<F: Fn(glib::Object) + 'static>(&self, f: F) {
        self.obj.connect_local("object-added", false, move |args| {
            if let Some(obj) = args.get(1).and_then(|v| v.get::<glib::Object>().ok()) {
                f(obj);
            }
            None
        });
    }

    pub fn connect_object_removed<F: Fn(glib::Object) + 'static>(&self, f: F) {
        self.obj.connect_local("object-removed", false, move |args| {
            if let Some(obj) = args.get(1).and_then(|v| v.get::<glib::Object>().ok()) {
                f(obj);
            }
            None
        });
    }

    // Fires once the initial set of matched objects have settled
    pub fn connect_installed<F: Fn() + 'static>(&self, f: F) {
        self.obj.connect_local("installed", false, move |_args| {
            f();
            None
        });
    }
}

// Node proxy from the OM
pub struct Node {
    obj:      glib::Object,
    channels: u32,
}

impl Node {
    pub fn from_object(obj: glib::Object) -> Self {
        let channels = node_pw_prop(&obj, "audio.channels")
            .and_then(|s| s.parse().ok())
            .unwrap_or(2);
        Node { obj, channels }
    }

    pub fn set_volume(&self, volume: f32) {
        let pod = unsafe { build_volume_pod(volume, self.channels) };
        if pod.is_null() { return; }
        let id = CString::new("Props").unwrap();
        unsafe {
            // set_param takes ownership of the pod
            ffi::wp_pipewire_object_set_param(
                self.obj.as_ptr() as *mut ffi::WpPipewireObject,
                id.as_ptr(),
                0,
                pod,
            );
        }
    }
}

// Wrapper over WpMetadata
pub struct Metadata {
    obj: glib::Object,
}

impl Metadata {
    pub fn from_object(obj: glib::Object) -> Self {
        Metadata { obj }
    }

    // Set the user configured default sink by node.name. We write the
    // configured key; WP resolves it to the effective default.audio.sink.
    pub fn set_default_sink(&self, name: &str) {
        let key   = CString::new("default.configured.audio.sink").unwrap();
        let type_ = CString::new("Spa:String:JSON").unwrap();
        let value = CString::new(format!("{{\"name\":\"{name}\"}}")).unwrap();

        unsafe {
            ffi::wp_metadata_set(
                self.obj.as_ptr() as *mut ffi::WpMetadata,
                0,
                key.as_ptr(),
                type_.as_ptr(),
                value.as_ptr(),
            );
        }
    }

    // Read a value from the local cache
    pub fn find(&self, subject: u32, key: &str) -> Option<String> {
        let key_c = CString::new(key).ok()?;
        unsafe {
            let val = ffi::wp_metadata_find(
                self.obj.as_ptr() as *mut ffi::WpMetadata,
                subject,
                key_c.as_ptr(),
                std::ptr::null_mut(),
            );
            if val.is_null() {
                None
            } else {
                CStr::from_ptr(val).to_str().ok().map(str::to_owned)
            }
        }
    }

    pub fn activate_data<F: FnOnce(bool) + 'static>(&self, on_ready: F) {
        let boxed: Box<Box<dyn FnOnce(bool)>> = Box::new(Box::new(on_ready));
        let data = Box::into_raw(boxed) as *mut c_void;
        unsafe {
            ffi::wp_object_activate(
                self.obj.as_ptr() as *mut ffi::WpObject,
                ffi::WP_METADATA_FEATURE_DATA,
                std::ptr::null_mut(),
                Some(activate_data_ready),
                data,
            );
        }
    }

    // changed(subject, key, type, value); key/value are None when an entry clears
    pub fn connect_changed<F: Fn(u32, Option<String>, Option<String>) + 'static>(&self, f: F) {
        self.obj.connect_local("changed", false, move |args| {
            let subject = args.get(1).and_then(|v| v.get::<u32>().ok()).unwrap_or(0);
            let key   = args.get(2).and_then(|v| v.get::<Option<String>>().ok()).flatten();
            // args.get(3) isn't needed
            let value = args.get(4).and_then(|v| v.get::<Option<String>>().ok()).flatten();
            f(subject, key, value);
            None
        });
    }
}

// Async; finishes activation and returns success flag to boxed FnOnce
unsafe extern "C" fn activate_data_ready(source: *mut c_void, res: *mut c_void, data: *mut c_void) {
    let mut err: *mut c_void = std::ptr::null_mut();
    let ok = ffi::wp_object_activate_finish(source as *mut ffi::WpObject, res, &mut err);
    if !err.is_null() {
        glib::ffi::g_error_free(err as *mut glib::ffi::GError);
    }
    let cb: Box<Box<dyn FnOnce(bool)>> = Box::from_raw(data as *mut Box<dyn FnOnce(bool)>);
    cb(ok != 0);
}

// caller takes ownership of the pod
// sets channelVolumes
// We have to build the array manually vs. using wp_spa_pod_new_object
// The channel count is only known at runtime
unsafe fn build_volume_pod(volume: f32, channels: u32) -> *mut ffi::WpSpaPod {
    let type_name = CString::new("Spa:Pod:Object:Param:Props").unwrap();
    let id_name = CString::new("Props").unwrap();
    let builder = ffi::wp_spa_pod_builder_new_object(
        type_name.as_ptr(),
        id_name.as_ptr(),
    );

    if builder.is_null() {
        return std::ptr::null_mut();
    }

    let key = CString::new("channelVolumes").unwrap();
    ffi::wp_spa_pod_builder_add_property(builder, key.as_ptr());

    let arr = ffi::wp_spa_pod_builder_new_array();

    if arr.is_null() {
        ffi::wp_spa_pod_builder_unref(builder);
        return std::ptr::null_mut();
    }

    for _ in 0..channels.max(1) {
        ffi::wp_spa_pod_builder_add_float(arr, volume);
    }

    let arr_pod = ffi::wp_spa_pod_builder_end(arr);

    if arr_pod.is_null() {
        ffi::wp_spa_pod_builder_unref(builder);
        return std::ptr::null_mut();
    }

    ffi::wp_spa_pod_builder_add_pod(builder, arr_pod);
    ffi::wp_spa_pod_unref(arr_pod);

    ffi::wp_spa_pod_builder_end(builder)
}
