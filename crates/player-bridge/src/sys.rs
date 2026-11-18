//! Minimal hand-written libmpv C API surface, resolved at runtime with dlopen.

#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void};
use std::sync::OnceLock;

#[repr(C)]
pub struct mpv_handle {
    _private: [u8; 0],
}

pub const FORMAT_NONE: c_int = 0;
pub const FORMAT_STRING: c_int = 1;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_INT64: c_int = 4;
pub const FORMAT_DOUBLE: c_int = 5;
pub const FORMAT_NODE: c_int = 6;
pub const FORMAT_NODE_ARRAY: c_int = 7;
pub const FORMAT_NODE_MAP: c_int = 8;
pub const FORMAT_BYTE_ARRAY: c_int = 9;

#[repr(C)]
pub struct mpv_byte_array {
    pub data: *mut c_void,
    pub size: usize,
}

#[repr(C)]
pub union mpv_node_union {
    pub string: *mut c_char,
    pub flag: c_int,
    pub int64: i64,
    pub double_: f64,
    pub list: *mut mpv_node_list,
    pub ba: *mut c_void,
}

#[repr(C)]
pub struct mpv_node {
    pub u: mpv_node_union,
    pub format: c_int,
}

#[repr(C)]
pub struct mpv_node_list {
    pub num: c_int,
    pub values: *mut mpv_node,
    pub keys: *mut *mut c_char,
}

pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_LOG_MESSAGE: c_int = 2;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;
pub const EVENT_PLAYBACK_RESTART: c_int = 21;
pub const EVENT_PROPERTY_CHANGE: c_int = 22;

pub const END_FILE_REASON_EOF: c_int = 0;
pub const END_FILE_REASON_STOP: c_int = 2;
pub const END_FILE_REASON_QUIT: c_int = 3;
pub const END_FILE_REASON_ERROR: c_int = 4;

pub const ERROR_LOADING_FAILED: c_int = -13;
pub const ERROR_GENERIC: i64 = -20;

#[repr(C)]
pub struct mpv_event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_property {
    pub name: *const c_char,
    pub format: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_end_file {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

#[repr(C)]
pub struct mpv_event_log_message {
    pub prefix: *const c_char,
    pub level: *const c_char,
    pub text: *const c_char,
    pub log_level: c_int,
}

pub type ReadFn = unsafe extern "C" fn(cookie: *mut c_void, buf: *mut c_char, nbytes: u64) -> i64;
pub type SeekFn = unsafe extern "C" fn(cookie: *mut c_void, offset: i64) -> i64;
pub type SizeFn = unsafe extern "C" fn(cookie: *mut c_void) -> i64;
pub type CloseFn = unsafe extern "C" fn(cookie: *mut c_void);
pub type CancelFn = unsafe extern "C" fn(cookie: *mut c_void);

#[repr(C)]
pub struct mpv_stream_cb_info {
    pub cookie: *mut c_void,
    pub read_fn: Option<ReadFn>,
    pub seek_fn: Option<SeekFn>,
    pub size_fn: Option<SizeFn>,
    pub close_fn: Option<CloseFn>,
    pub cancel_fn: Option<CancelFn>,
}

#[repr(C)]
pub struct mpv_render_context {
    _private: [u8; 0],
}

#[repr(C)]
pub struct mpv_render_param {
    pub kind: c_int,
    pub data: *mut c_void,
}

pub const RENDER_PARAM_INVALID: c_int = 0;
pub const RENDER_PARAM_API_TYPE: c_int = 1;
pub const RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
pub const RENDER_PARAM_OPENGL_FBO: c_int = 3;
pub const RENDER_PARAM_FLIP_Y: c_int = 4;
pub const RENDER_PARAM_ADVANCED_CONTROL: c_int = 10;
pub const RENDER_PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
pub const RENDER_UPDATE_FRAME: u64 = 1;

pub type GetProcAddressFn =
    unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void;
pub type UpdateFn = unsafe extern "C" fn(ctx: *mut c_void);

#[repr(C)]
pub struct mpv_opengl_init_params {
    pub get_proc_address: Option<GetProcAddressFn>,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
pub struct mpv_opengl_fbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

pub type OpenFn = unsafe extern "C" fn(
    user_data: *mut c_void,
    uri: *mut c_char,
    info: *mut mpv_stream_cb_info,
) -> c_int;

pub struct Api {
    _lib: libloading::Library,
    pub create: unsafe extern "C" fn() -> *mut mpv_handle,
    pub initialize: unsafe extern "C" fn(*mut mpv_handle) -> c_int,
    pub set_option_string:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub set_property_string:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub get_property:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub get_property_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char) -> *mut c_char,
    pub free_node_contents: unsafe extern "C" fn(*mut mpv_node),
    pub free: unsafe extern "C" fn(*mut c_void),
    pub command_async: unsafe extern "C" fn(*mut mpv_handle, u64, *mut *const c_char) -> c_int,
    pub command_ret:
        unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char, *mut mpv_node) -> c_int,
    pub observe_property: unsafe extern "C" fn(*mut mpv_handle, u64, *const c_char, c_int) -> c_int,
    pub request_log_messages: unsafe extern "C" fn(*mut mpv_handle, *const c_char) -> c_int,
    pub wait_event: unsafe extern "C" fn(*mut mpv_handle, f64) -> *mut mpv_event,
    pub terminate_destroy: unsafe extern "C" fn(*mut mpv_handle),
    pub error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    pub stream_cb_add_ro:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, *mut c_void, OpenFn) -> c_int,
    pub render_context_create: unsafe extern "C" fn(
        *mut *mut mpv_render_context,
        *mut mpv_handle,
        *mut mpv_render_param,
    ) -> c_int,
    pub render_context_set_update_callback:
        unsafe extern "C" fn(*mut mpv_render_context, Option<UpdateFn>, *mut c_void),
    pub render_context_update: unsafe extern "C" fn(*mut mpv_render_context) -> u64,
    pub render_context_render:
        unsafe extern "C" fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int,
    pub render_context_free: unsafe extern "C" fn(*mut mpv_render_context),
}

const DEFAULT_PATHS: &[&str] = &[
    "/opt/homebrew/lib/libmpv.2.dylib",
    "/usr/local/lib/libmpv.2.dylib",
];

static API: OnceLock<Result<Api, String>> = OnceLock::new();

/// Loads libmpv once per process (first playback).
pub fn api() -> Result<&'static Api, String> {
    API.get_or_init(load).as_ref().map_err(Clone::clone)
}

fn load() -> Result<Api, String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(path) = std::env::var("CINMA_LIBMPV") {
        candidates.push(path);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(macos_dir) = exe.parent()
        && let Some(contents_dir) = macos_dir.parent()
    {
        candidates.push(
            contents_dir
                .join("Frameworks/libmpv.2.dylib")
                .to_string_lossy()
                .into_owned(),
        );
    }
    candidates.extend(DEFAULT_PATHS.iter().map(|s| (*s).to_owned()));
    let mut errors = Vec::new();
    for path in &candidates {
        // SAFETY: loading libmpv runs its (well-behaved) initializers.
        match unsafe { libloading::Library::new(path) } {
            Ok(lib) => return resolve(lib).map_err(|e| format!("{path}: {e}")),
            Err(e) => errors.push(format!("{path}: {e}")),
        }
    }
    Err(format!("libmpv not found ({})", errors.join("; ")))
}

fn resolve(lib: libloading::Library) -> Result<Api, libloading::Error> {
    // SAFETY: symbol types match libmpv's client.h / stream_cb.h (API 2.x).
    unsafe {
        Ok(Api {
            create: *lib.get(b"mpv_create\0")?,
            initialize: *lib.get(b"mpv_initialize\0")?,
            set_option_string: *lib.get(b"mpv_set_option_string\0")?,
            set_property_string: *lib.get(b"mpv_set_property_string\0")?,
            get_property: *lib.get(b"mpv_get_property\0")?,
            get_property_string: *lib.get(b"mpv_get_property_string\0")?,
            free_node_contents: *lib.get(b"mpv_free_node_contents\0")?,
            free: *lib.get(b"mpv_free\0")?,
            command_async: *lib.get(b"mpv_command_async\0")?,
            command_ret: *lib.get(b"mpv_command_ret\0")?,
            observe_property: *lib.get(b"mpv_observe_property\0")?,
            request_log_messages: *lib.get(b"mpv_request_log_messages\0")?,
            wait_event: *lib.get(b"mpv_wait_event\0")?,
            terminate_destroy: *lib.get(b"mpv_terminate_destroy\0")?,
            error_string: *lib.get(b"mpv_error_string\0")?,
            stream_cb_add_ro: *lib.get(b"mpv_stream_cb_add_ro\0")?,
            render_context_create: *lib.get(b"mpv_render_context_create\0")?,
            render_context_set_update_callback: *lib
                .get(b"mpv_render_context_set_update_callback\0")?,
            render_context_update: *lib.get(b"mpv_render_context_update\0")?,
            render_context_render: *lib.get(b"mpv_render_context_render\0")?,
            render_context_free: *lib.get(b"mpv_render_context_free\0")?,
            _lib: lib,
        })
    }
}
