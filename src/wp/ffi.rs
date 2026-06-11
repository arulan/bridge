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

use std::ffi::{c_char, c_void};
use glib::ffi::gboolean;

// gsize
pub type GType = usize;

// wp types
#[repr(C)] pub struct WpCore(u8);
#[repr(C)] pub struct WpConf(u8);
#[repr(C)] pub struct WpObjectManager(u8);
#[repr(C)] pub struct WpObjectInterest(u8);
#[repr(C)] pub struct WpProperties(u8);
#[repr(C)] pub struct WpProxy(u8);
#[repr(C)] pub struct WpGlobalProxy(u8);
#[repr(C)] pub struct WpPipewireObject(u8);
#[repr(C)] pub struct WpSpaPod(u8);
#[repr(C)] pub struct WpSpaPodBuilder(u8);
#[repr(C)] pub struct WpMetadata(u8);
#[repr(C)] pub struct WpObject(u8);

pub const WP_INIT_ALL: u32 = 15;
pub const WP_PROXY_FEATURE_BOUND: u32 = 1;
pub const WP_PIPEWIRE_OBJECT_FEATURE_INFO: u32 = 16;

// TODO: Necessary to use Async to avoid freezes?
pub const WP_METADATA_FEATURE_DATA: u32 = 1 << 16;

#[link(name = "wireplumber-0.5")]
extern "C" {
    pub fn wp_init(flags: u32);

    pub fn wp_core_new(
        main_context: *mut glib::ffi::GMainContext,
        conf: *mut WpConf,
        properties: *mut WpProperties,
    ) -> *mut WpCore;

    pub fn wp_core_connect(self_: *mut WpCore) -> gboolean;
    pub fn wp_core_disconnect(self_: *mut WpCore);
    pub fn wp_core_install_object_manager(self_: *mut WpCore, om: *mut WpObjectManager);

    pub fn wp_object_manager_new() -> *mut WpObjectManager;
    pub fn wp_object_manager_add_interest_full(
        self_: *mut WpObjectManager,
        interest: *mut WpObjectInterest,
    );

    pub fn wp_object_manager_request_object_features(
        self_: *mut WpObjectManager,
        object_type: GType,
        requested_features: u32,
    );

    pub fn wp_object_interest_new_type(gtype: GType) -> *mut WpObjectInterest;

    pub fn wp_proxy_get_bound_id(self_: *mut WpProxy) -> u32;

    pub fn wp_global_proxy_get_global_properties(
        self_: *mut WpGlobalProxy,
    ) -> *const WpProperties;
    
    pub fn wp_properties_get(self_: *const WpProperties, key: *const c_char) -> *const c_char;

    pub fn wp_node_get_type() -> GType;
    pub fn wp_metadata_get_type() -> GType;

    // Read a metadata value from the local cache
    pub fn wp_metadata_find(
        self_:   *mut WpMetadata,
        subject: u32,
        key:     *const c_char,
        type_:   *mut *const c_char,
    ) -> *const c_char;

    pub fn wp_metadata_set(
        self_:   *mut WpMetadata,
        subject: u32,
        key:     *const c_char,
        type_:   *const c_char,
        value:   *const c_char,
    );

    pub fn wp_object_activate(
        self_:       *mut WpObject,
        features:    u32,
        cancellable: *mut c_void,
        callback:    Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void)>,
        user_data:   *mut c_void,
    );
    pub fn wp_object_activate_finish(
        self_: *mut WpObject,
        res:   *mut c_void,
        error: *mut *mut c_void,
    ) -> i32;

    // Requires WP_PIPEWIRE_OBJECT_FEATURE_INFO
    pub fn wp_pipewire_object_get_property(
        self_: *mut WpPipewireObject,
        key:   *const c_char,
    ) -> *const c_char;

    pub fn wp_pipewire_object_set_param(
        self_: *mut WpPipewireObject,
        id:    *const c_char,
        flags: u32,
        param: *mut WpSpaPod,
    ) -> gboolean;

    pub fn wp_spa_pod_builder_new_object(
        type_name: *const c_char,
        id_name:   *const c_char,
    ) -> *mut WpSpaPodBuilder;

    pub fn wp_spa_pod_builder_new_array() -> *mut WpSpaPodBuilder;

    pub fn wp_spa_pod_builder_add_property(
        self_: *mut WpSpaPodBuilder,
        key:   *const c_char,
    );

    pub fn wp_spa_pod_builder_add_float(
        self_: *mut WpSpaPodBuilder,
        value: f32,
    );

    pub fn wp_spa_pod_builder_add_pod(
        self_: *mut WpSpaPodBuilder,
        pod:   *mut WpSpaPod,
    );

    pub fn wp_spa_pod_builder_end(
        self_: *mut WpSpaPodBuilder,
    ) -> *mut WpSpaPod;
    
    pub fn wp_spa_pod_builder_unref(self_: *mut WpSpaPodBuilder);
    pub fn wp_spa_pod_unref(self_: *mut WpSpaPod);
}
