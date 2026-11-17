//! libmpv render API (OpenGL) — mpv draws into a GL framebuffer owned by the
//! host's `CAOpenGLLayer` (docs/adr/0004). Every call here must happen on the
//! thread where that layer's CGL context is current.

use std::ffi::{CStr, c_char, c_void};
use std::sync::{Arc, OnceLock};

use crate::sys;

pub type UpdateCallback = Arc<dyn Fn() + Send + Sync>;

pub(crate) struct Renderer {
    pub ctx: *mut sys::mpv_render_context,
    /// Leaked `Box<UpdateCallback>`; reclaimed after the context is freed.
    pub callback: *mut UpdateCallback,
}

// SAFETY: the pointers are only dereferenced by mpv / in `free`, which the
// caller serializes through the Player's mutex.
unsafe impl Send for Renderer {}

const OPENGL_FRAMEWORK: &str = "/System/Library/Frameworks/OpenGL.framework/OpenGL";

fn opengl() -> Option<&'static libloading::Library> {
    static LIB: OnceLock<Option<libloading::Library>> = OnceLock::new();
    // SAFETY: loading a system framework.
    LIB.get_or_init(|| unsafe { libloading::Library::new(OPENGL_FRAMEWORK) }.ok())
        .as_ref()
}

pub(crate) unsafe extern "C" fn get_proc_address(
    _ctx: *mut c_void,
    name: *const c_char,
) -> *mut c_void {
    let Some(lib) = opengl() else {
        return std::ptr::null_mut();
    };
    // SAFETY: mpv passes a NUL-terminated symbol name.
    let name = unsafe { CStr::from_ptr(name) };
    // SAFETY: we only read the symbol's address.
    match unsafe { lib.get::<*mut c_void>(name.to_bytes_with_nul()) } {
        Ok(symbol) => *symbol,
        Err(_) => std::ptr::null_mut(),
    }
}

pub(crate) unsafe extern "C" fn on_update(ctx: *mut c_void) {
    let _ = std::panic::catch_unwind(|| {
        // SAFETY: ctx is the leaked Box<UpdateCallback>, alive until free.
        let callback = unsafe { &*(ctx as *const UpdateCallback) };
        callback();
    });
}
