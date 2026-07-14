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

// What remains of FFI, libpipewire only; Needed for the temp sinks

use std::ffi::{CString, c_char, c_void};
use std::ptr::NonNull;

use pipewire::context::ContextRc;

pub(super) fn load_module(context: &ContextRc, name: &str, args: &str) -> Option<LoadedModule> {
    let name_c = CString::new(name).ok()?;
    let args_c = CString::new(args).ok()?;
    unsafe {
        let m = pw_context_load_module(
            context.as_raw_ptr() as *mut PwContext,
            name_c.as_ptr(),
            args_c.as_ptr(),
            std::ptr::null_mut(),
        );
        NonNull::new(m).map(LoadedModule)
    }
}

pub(super) struct LoadedModule(NonNull<PwImplModule>);

impl Drop for LoadedModule {
    fn drop(&mut self) {
        unsafe { pw_impl_module_destroy(self.0.as_ptr()) }
    }
}

#[repr(C)]
struct PwContext(u8);
#[repr(C)]
struct PwImplModule(u8);

#[link(name = "pipewire-0.3")]
unsafe extern "C" {
    fn pw_context_load_module(
        context: *mut PwContext,
        name: *const c_char,
        args: *const c_char,
        properties: *mut c_void,
    ) -> *mut PwImplModule;

    fn pw_impl_module_destroy(module: *mut PwImplModule);
}
