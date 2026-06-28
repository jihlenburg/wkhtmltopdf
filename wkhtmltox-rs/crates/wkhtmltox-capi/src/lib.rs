// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// wkhtmltox-capi/src/lib.rs — C ABI exports for libwkhtmltox (PDF surface).
//
// Every exported `extern "C"` function wraps its body in
// `std::panic::catch_unwind` and returns a safe default (0 / null / etc.) on
// panic — no panic may cross the FFI boundary.  Raw pointer arguments are
// null-checked before dereference.

#![allow(clippy::missing_safety_doc)]

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_long, c_uchar};

use wkhtmltox_core::assembly::{assemble_pdf, AssembleOpts};
use wkhtmltox_core::image::produce as image_produce;
use wkhtmltox_core::registry::{
    get_global, get_image_global, get_object, set_global, set_image_global, set_object,
};
use wkhtmltox_core::render::{ReadyPolicy, Renderer, SnapshotOpts, Source};
use wkhtmltox_core::settings::{GlobalSettings, ImageGlobalSettings, PdfObjectSettings};
use wkhtmltox_render_chromium::renderer::{ChromiumRenderer, SpawnOpts};

// ---------------------------------------------------------------------------
// Opaque handle type aliases
//
// C consumers see these as forward-declared opaque structs.  Internally they
// are our Rust structs, vended as raw Box pointers via `Box::into_raw`.
// ---------------------------------------------------------------------------

/// Opaque type behind `wkhtmltopdf_global_settings *`.
pub type WkGlobalSettings = GlobalSettings;

/// Opaque type behind `wkhtmltopdf_object_settings *`.
pub type WkObjectSettings = PdfObjectSettings;

// ---------------------------------------------------------------------------
// C callback type aliases (mirror the typedefs in pdf.h)
// ---------------------------------------------------------------------------

/// `void (*)(wkhtmltopdf_converter *, const char *)`
pub type WkhtmltopdfStrCallback = Option<unsafe extern "C" fn(*mut CConverter, *const c_char)>;
/// `void (*)(wkhtmltopdf_converter *, const int)`
pub type WkhtmltopdfIntCallback = Option<unsafe extern "C" fn(*mut CConverter, c_int)>;
/// `void (*)(wkhtmltopdf_converter *)`
pub type WkhtmltopdfVoidCallback = Option<unsafe extern "C" fn(*mut CConverter)>;

// ---------------------------------------------------------------------------
// Phase names — must match upstream wkhtmltopdf exactly.
// ---------------------------------------------------------------------------

const PHASES: &[&str] = &[
    "Loading pages",
    "Counting pages",
    "Resolving links",
    "Loading headers and footers",
    "Printing pages",
    "Done",
];

// ---------------------------------------------------------------------------
// CConverter — internal state behind `wkhtmltopdf_converter *`
//
// # Safety and threading
//
// A single `CConverter` handle (and the raw pointer returned by
// `wkhtmltopdf_create_converter`) must be used from **one thread at a time**.
// Its internal fields (`str_cache`, `output`, `objects`) are mutated through
// the handle and are not synchronised.  Distinct converters are independent
// and may be used concurrently from different threads.
//
// This matches upstream wkhtmltopdf's single-threaded-converter contract.
// ---------------------------------------------------------------------------

pub struct CConverter {
    global: GlobalSettings,
    objects: Vec<(PdfObjectSettings, Option<String>)>,
    // Callbacks
    warning_cb: WkhtmltopdfStrCallback,
    error_cb: WkhtmltopdfStrCallback,
    phase_changed_cb: WkhtmltopdfVoidCallback,
    progress_changed_cb: WkhtmltopdfIntCallback,
    finished_cb: WkhtmltopdfIntCallback,
    // Output PDF bytes (borrowed by wkhtmltopdf_get_output until destroyed).
    output: Vec<u8>,
    http_error: i32,
    // Phase/progress tracking.
    cur_phase: i32,
    phase_count: i32,
    /// Human-readable progress string, updated during convert.
    progress_str: String,
    /// Interned C strings.  All `const char*` return values point into here and
    /// are valid for the converter's lifetime — this is the designed-out
    /// dangling-pointer bug in the original wkhtmltopdf C API.
    str_cache: HashMap<String, CString>,
}

impl CConverter {
    fn new(global: GlobalSettings) -> Self {
        Self {
            global,
            objects: Vec::new(),
            warning_cb: None,
            error_cb: None,
            phase_changed_cb: None,
            progress_changed_cb: None,
            finished_cb: None,
            output: Vec::new(),
            http_error: 0,
            cur_phase: -1,
            phase_count: PHASES.len() as i32,
            progress_str: String::new(),
            str_cache: HashMap::new(),
        }
    }

    /// Intern `s` in the cache and return a pointer valid for `&self`'s lifetime.
    fn cache_str(&mut self, s: &str) -> *const c_char {
        self.str_cache
            .entry(s.to_owned())
            .or_insert_with(|| {
                // Strip NUL bytes (shouldn't appear in practice).
                let bytes: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
                // SAFETY: all interior NUL bytes were removed above.
                unsafe { CString::from_vec_unchecked(bytes) }
            })
            .as_ptr()
    }
}

// ---------------------------------------------------------------------------
// Internal callback helpers — all `unsafe` because they dereference raw ptrs
// and invoke C function pointers.
// ---------------------------------------------------------------------------

/// Update `cur_phase` and fire the `phase_changed` callback.
unsafe fn phase_emit(conv: *mut CConverter, phase: i32) {
    (*conv).cur_phase = phase;
    let desc = PHASES.get(phase as usize).copied().unwrap_or("");
    (*conv).progress_str = desc.to_owned();
    if let Some(cb) = (*conv).phase_changed_cb {
        cb(conv);
    }
}

/// Fire the `progress_changed` callback and update the progress string.
unsafe fn progress_emit(conv: *mut CConverter, pct: c_int) {
    let phase = (*conv).cur_phase;
    let desc = PHASES.get(phase.max(0) as usize).copied().unwrap_or("");
    (*conv).progress_str = format!("{desc} [{pct}%]");
    if let Some(cb) = (*conv).progress_changed_cb {
        cb(conv, pct);
    }
}

/// Fire the `error` callback with a Rust string message.
unsafe fn error_emit(conv: *mut CConverter, msg: &str) {
    if let Some(cb) = (*conv).error_cb {
        let bytes: Vec<u8> = msg.bytes().filter(|&b| b != 0).collect();
        // SAFETY: NUL bytes removed above.
        let cs = CString::from_vec_unchecked(bytes);
        cb(conv, cs.as_ptr());
    }
}

/// Fire the `finished` callback.
unsafe fn finished_emit(conv: *mut CConverter, ok: c_int) {
    if let Some(cb) = (*conv).finished_cb {
        cb(conv, ok);
    }
}

// ---------------------------------------------------------------------------
// Static strings
// ---------------------------------------------------------------------------

/// Static NUL-terminated version string; `wkhtmltopdf_version()` returns a
/// pointer into this.
static VERSION_STR: &[u8] = concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes();
static EMPTY_CSTR: &[u8] = b"\0";

#[inline(always)]
fn empty_cstr() -> *const c_char {
    EMPTY_CSTR.as_ptr() as *const c_char
}

// ---------------------------------------------------------------------------
// Helper: write a Rust `&str` into a caller-supplied `char*` buffer.
//
// Copies at most `vs - 1` bytes, then NUL-terminates.
// Returns 1 (the setting was known), so callers can forward this return value.
// ---------------------------------------------------------------------------
unsafe fn write_to_buf(src: &str, buf: *mut c_char, vs: c_int) -> c_int {
    if buf.is_null() || vs <= 0 {
        return 1; // known, but no room to write — not an error
    }
    let cap = (vs as usize).saturating_sub(1); // reserve one byte for NUL
    let bytes = src.as_bytes();
    let n = bytes.len().min(cap);
    std::ptr::copy_nonoverlapping(bytes.as_ptr().cast::<c_char>(), buf, n);
    *buf.add(n) = 0; // NUL-terminate
    1
}

// ===========================================================================
// Exported C ABI functions
// ===========================================================================

/// `wkhtmltopdf_init(int use_graphics)` → 1.
///
/// No-op in our implementation — no Qt process to initialise.
#[no_mangle]
pub extern "C" fn wkhtmltopdf_init(_use_graphics: c_int) -> c_int {
    std::panic::catch_unwind(|| 1).unwrap_or(0)
}

/// `wkhtmltopdf_deinit()` → 1.
#[no_mangle]
pub extern "C" fn wkhtmltopdf_deinit() -> c_int {
    std::panic::catch_unwind(|| 1).unwrap_or(0)
}

/// `wkhtmltopdf_extended_qt()` → 1.
///
/// Returns 1 to signal that extended capabilities (background printing,
/// JavaScript, etc.) are available.  We provide them via Chromium rather than
/// the patched-Qt4 build that the original wkhtmltopdf required.
#[no_mangle]
pub extern "C" fn wkhtmltopdf_extended_qt() -> c_int {
    std::panic::catch_unwind(|| 1).unwrap_or(0)
}

/// `wkhtmltopdf_version()` → `const char*`.
///
/// Points to a `'static` NUL-terminated byte string — always valid.
#[no_mangle]
pub extern "C" fn wkhtmltopdf_version() -> *const c_char {
    std::panic::catch_unwind(|| VERSION_STR.as_ptr() as *const c_char).unwrap_or(empty_cstr())
}

// ---------------------------------------------------------------------------
// Global settings
// ---------------------------------------------------------------------------

/// `wkhtmltopdf_create_global_settings()` → opaque handle.
#[no_mangle]
pub extern "C" fn wkhtmltopdf_create_global_settings() -> *mut WkGlobalSettings {
    std::panic::catch_unwind(|| Box::into_raw(Box::new(GlobalSettings::default())))
        .unwrap_or(std::ptr::null_mut())
}

/// `wkhtmltopdf_destroy_global_settings(settings)`.
///
/// # Safety
/// `settings` must be a live pointer previously returned by
/// `wkhtmltopdf_create_global_settings` and not yet destroyed.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_destroy_global_settings(settings: *mut WkGlobalSettings) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !settings.is_null() {
            drop(Box::from_raw(settings));
        }
    }));
}

/// `wkhtmltopdf_set_global_setting(settings, name, value)` → 1 ok, 0 err.
///
/// # Safety
/// All pointer arguments must be valid (or null — null returns 0).
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_set_global_setting(
    settings: *mut WkGlobalSettings,
    name: *const c_char,
    value: *const c_char,
) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if settings.is_null() || name.is_null() || value.is_null() {
            return 0;
        }
        let g = &mut *settings;
        let name_s = CStr::from_ptr(name).to_str().unwrap_or("");
        let value_s = CStr::from_ptr(value).to_str().unwrap_or("");
        match set_global(g, name_s, value_s) {
            Ok(()) => 1,
            Err(_) => 0,
        }
    }))
    .unwrap_or(0)
}

/// `wkhtmltopdf_get_global_setting(settings, name, value, vs)` → 1 known, 0 unknown.
///
/// # Safety
/// `settings` and `name` must be valid.  `value` may be null (silently skipped).
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_get_global_setting(
    settings: *mut WkGlobalSettings,
    name: *const c_char,
    value: *mut c_char,
    vs: c_int,
) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if settings.is_null() || name.is_null() {
            return 0;
        }
        let g = &*settings;
        let name_s = CStr::from_ptr(name).to_str().unwrap_or("");
        match get_global(g, name_s) {
            Some(s) => write_to_buf(&s, value, vs),
            None => 0,
        }
    }))
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Object settings
// ---------------------------------------------------------------------------

/// `wkhtmltopdf_create_object_settings()` → opaque handle.
#[no_mangle]
pub extern "C" fn wkhtmltopdf_create_object_settings() -> *mut WkObjectSettings {
    std::panic::catch_unwind(|| Box::into_raw(Box::new(PdfObjectSettings::default())))
        .unwrap_or(std::ptr::null_mut())
}

/// `wkhtmltopdf_destroy_object_settings(settings)`.
///
/// # Safety
/// `settings` must be a live pointer not yet destroyed.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_destroy_object_settings(settings: *mut WkObjectSettings) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !settings.is_null() {
            drop(Box::from_raw(settings));
        }
    }));
}

/// `wkhtmltopdf_set_object_setting(settings, name, value)` → 1 ok, 0 err.
///
/// # Safety
/// All pointer arguments must be valid (or null — null returns 0).
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_set_object_setting(
    settings: *mut WkObjectSettings,
    name: *const c_char,
    value: *const c_char,
) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if settings.is_null() || name.is_null() || value.is_null() {
            return 0;
        }
        let o = &mut *settings;
        let name_s = CStr::from_ptr(name).to_str().unwrap_or("");
        let value_s = CStr::from_ptr(value).to_str().unwrap_or("");
        match set_object(o, name_s, value_s) {
            Ok(()) => 1,
            Err(_) => 0,
        }
    }))
    .unwrap_or(0)
}

/// `wkhtmltopdf_get_object_setting(settings, name, value, vs)` → 1 known, 0 unknown.
///
/// # Safety
/// `settings` and `name` must be valid.  `value` may be null.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_get_object_setting(
    settings: *mut WkObjectSettings,
    name: *const c_char,
    value: *mut c_char,
    vs: c_int,
) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if settings.is_null() || name.is_null() {
            return 0;
        }
        let o = &*settings;
        let name_s = CStr::from_ptr(name).to_str().unwrap_or("");
        match get_object(o, name_s) {
            Some(s) => write_to_buf(&s, value, vs),
            None => 0,
        }
    }))
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Converter lifecycle
// ---------------------------------------------------------------------------

/// `wkhtmltopdf_create_converter(settings)` → converter handle.
///
/// # Ownership
/// **Transfers** ownership of `settings` to the returned converter.  The caller
/// must not use or destroy `settings` after this call.  `wkhtmltopdf_destroy_converter`
/// will free the converter together with its owned `GlobalSettings`.
///
/// This matches upstream wkhtmltopdf's `pdf.h` semantics, where
/// `wkhtmltopdf_destroy_converter` is responsible for freeing the settings.
///
/// # Safety
/// `settings` must be a live pointer returned by
/// `wkhtmltopdf_create_global_settings`, or null.  If null, null is returned
/// and no settings are consumed.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_create_converter(
    settings: *mut WkGlobalSettings,
) -> *mut CConverter {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if settings.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: Caller guarantees `settings` is a live pointer from
        // `wkhtmltopdf_create_global_settings`.  We take ownership here;
        // the caller must not use or destroy `settings` after this point.
        let gs = *Box::from_raw(settings);
        Box::into_raw(Box::new(CConverter::new(gs)))
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// `wkhtmltopdf_destroy_converter(converter)`.
///
/// # Safety
/// `converter` must be a live pointer not yet destroyed, or null.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_destroy_converter(converter: *mut CConverter) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            drop(Box::from_raw(converter));
        }
    }));
}

// ---------------------------------------------------------------------------
// Callback setters
// ---------------------------------------------------------------------------

/// `wkhtmltopdf_set_warning_callback(converter, cb)`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_set_warning_callback(
    converter: *mut CConverter,
    cb: WkhtmltopdfStrCallback,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            (*converter).warning_cb = cb;
        }
    }));
}

/// `wkhtmltopdf_set_error_callback(converter, cb)`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_set_error_callback(
    converter: *mut CConverter,
    cb: WkhtmltopdfStrCallback,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            (*converter).error_cb = cb;
        }
    }));
}

/// `wkhtmltopdf_set_phase_changed_callback(converter, cb)`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_set_phase_changed_callback(
    converter: *mut CConverter,
    cb: WkhtmltopdfVoidCallback,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            (*converter).phase_changed_cb = cb;
        }
    }));
}

/// `wkhtmltopdf_set_progress_changed_callback(converter, cb)`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_set_progress_changed_callback(
    converter: *mut CConverter,
    cb: WkhtmltopdfIntCallback,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            (*converter).progress_changed_cb = cb;
        }
    }));
}

/// `wkhtmltopdf_set_finished_callback(converter, cb)`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_set_finished_callback(
    converter: *mut CConverter,
    cb: WkhtmltopdfIntCallback,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            (*converter).finished_cb = cb;
        }
    }));
}

// ---------------------------------------------------------------------------
// add_object
// ---------------------------------------------------------------------------

/// `wkhtmltopdf_add_object(converter, object_settings, data)`.
///
/// If `data` is non-null (and non-empty), it is used as inline HTML source.
/// If `data` is null, the `page` field from `object_settings` is used as a URL.
///
/// # Ownership
/// **Transfers** ownership of `object_settings` to the converter — the caller
/// must not use or destroy `object_settings` after this call.  The owned
/// `PdfObjectSettings` is freed when the converter is destroyed.
///
/// This matches upstream wkhtmltopdf's `pdf.h` semantics.
///
/// # Safety
/// `converter` and `object_settings` must be live pointers or null.  If either
/// is null the call is a no-op and ownership is NOT transferred.
/// `data` may be null.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_add_object(
    converter: *mut CConverter,
    object_settings: *mut WkObjectSettings,
    data: *const c_char,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() || object_settings.is_null() {
            return;
        }
        // SAFETY: Caller guarantees `object_settings` is a live pointer from
        // `wkhtmltopdf_create_object_settings`.  We take ownership here;
        // the caller must not use or destroy `object_settings` after this point.
        let os = *Box::from_raw(object_settings);
        let html: Option<String> = if data.is_null() {
            None
        } else {
            let s = CStr::from_ptr(data).to_string_lossy().into_owned();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        };
        (*converter).objects.push((os, html));
    }));
}

// ---------------------------------------------------------------------------
// convert — main pipeline
// ---------------------------------------------------------------------------

/// `wkhtmltopdf_convert(converter)` → 1 on success, 0 on failure.
///
/// Full pipeline:
///   1. Build `Vec<Source>` from added objects.
///   2. Derive `PageGeometry` and `AssembleOpts` from the global settings.
///   3. Spawn a `ChromiumRenderer`.
///   4. Call `assemble_pdf` → write to a temp file.
///   5. Read the assembled bytes into `converter.output`.
///   6. Drive phase/progress/finished callbacks.
///
/// On failure the `error` callback is called with a description, then
/// `finished(0)`.  `http_error_code` is set to 1 on assembly failure.
///
/// # Safety
/// `converter` must be a live pointer or null.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_convert(converter: *mut CConverter) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> c_int {
        if converter.is_null() {
            return 0;
        }

        // Extract source list without holding a reference across the unsafe calls.
        let sources: Vec<Source> = {
            let conv = &*converter;
            conv.objects
                .iter()
                .map(|(os, html)| match html {
                    Some(h) => Source::Html(h.clone()),
                    None => Source::Url(os.page.clone()),
                })
                .collect()
        };

        if sources.is_empty() {
            error_emit(converter, "no objects to convert");
            (*converter).http_error = 1;
            finished_emit(converter, 0);
            return 0;
        }

        let (geom, opts): (_, AssembleOpts) = {
            let conv = &*converter;
            (conv.global.to_geometry(), conv.global.to_assemble_opts())
        };

        // Phase 0: Loading pages
        phase_emit(converter, 0);
        progress_emit(converter, 0);

        // Spawn Chromium, forwarding the global proxy setting if set.
        let proxy = {
            let conv = &*converter;
            conv.global.proxy.clone()
        };
        let mut renderer = match ChromiumRenderer::spawn_opts(SpawnOpts { proxy }) {
            Ok(r) => r,
            Err(e) => {
                error_emit(converter, &format!("renderer spawn failed: {e}"));
                (*converter).http_error = -1;
                finished_emit(converter, 0);
                return 0;
            }
        };

        // Phase 1: Counting pages
        phase_emit(converter, 1);
        progress_emit(converter, 20);

        // Temporary output file.
        let tmp = match tempfile::NamedTempFile::new() {
            Ok(t) => t,
            Err(e) => {
                error_emit(converter, &format!("tempfile: {e}"));
                (*converter).http_error = 1;
                finished_emit(converter, 0);
                return 0;
            }
        };
        let tmp_path = tmp.path().to_path_buf();

        // Phase 2: Resolving links
        phase_emit(converter, 2);
        progress_emit(converter, 40);

        // Phase 3: Loading headers and footers
        phase_emit(converter, 3);
        progress_emit(converter, 60);

        // Phase 4: Printing pages
        phase_emit(converter, 4);
        progress_emit(converter, 80);

        match assemble_pdf(&mut renderer, &sources, &geom, &tmp_path, &opts) {
            Ok(_report) => {
                match std::fs::read(&tmp_path) {
                    Ok(bytes) => {
                        (*converter).output = bytes;
                        // Phase 5: Done
                        phase_emit(converter, 5);
                        progress_emit(converter, 100);
                        finished_emit(converter, 1);
                        1
                    }
                    Err(e) => {
                        error_emit(converter, &format!("read assembled output: {e}"));
                        (*converter).http_error = 1;
                        finished_emit(converter, 0);
                        0
                    }
                }
            }
            Err(e) => {
                error_emit(converter, &format!("assemble_pdf: {e}"));
                (*converter).http_error = 1;
                finished_emit(converter, 0);
                0
            }
        }
    }))
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Phase / progress accessors
// ---------------------------------------------------------------------------

/// `wkhtmltopdf_current_phase(converter)` → current phase index (0-based), or -1.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_current_phase(converter: *mut CConverter) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            return -1;
        }
        (*converter).cur_phase
    }))
    .unwrap_or(-1)
}

/// `wkhtmltopdf_phase_count(converter)` → total number of phases.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_phase_count(converter: *mut CConverter) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            return 0;
        }
        (*converter).phase_count
    }))
    .unwrap_or(0)
}

/// `wkhtmltopdf_phase_description(converter, phase)` → `const char*`.
///
/// The returned pointer is borrowed from the converter's string cache and is
/// valid until the converter is destroyed.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_phase_description(
    converter: *mut CConverter,
    phase: c_int,
) -> *const c_char {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            return empty_cstr();
        }
        let name = PHASES.get(phase as usize).copied().unwrap_or("");
        (*converter).cache_str(name)
    }))
    .unwrap_or(empty_cstr())
}

/// `wkhtmltopdf_progress_string(converter)` → `const char*`.
///
/// Returns a human-readable progress description (e.g. `"Printing pages [80%]"`).
/// Borrowed from the converter's string cache; valid until destroyed.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_progress_string(converter: *mut CConverter) -> *const c_char {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            return empty_cstr();
        }
        let s = (*converter).progress_str.clone();
        (*converter).cache_str(&s)
    }))
    .unwrap_or(empty_cstr())
}

/// `wkhtmltopdf_http_error_code(converter)` → HTTP error code (0 = none).
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_http_error_code(converter: *mut CConverter) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            return 0;
        }
        (*converter).http_error
    }))
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// `wkhtmltopdf_get_output(converter, out)` → output length in bytes.
///
/// Sets `*out` to point at the start of the assembled PDF bytes stored inside
/// the converter.  The pointer is valid until the converter is destroyed or
/// `wkhtmltopdf_convert` is called again.
///
/// # Safety
/// `converter` must be live or null.  `out` may be null.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltopdf_get_output(
    converter: *mut CConverter,
    out: *mut *const c_uchar,
) -> c_long {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            if !out.is_null() {
                *out = std::ptr::null();
            }
            return 0;
        }
        let conv = &*converter;
        if !out.is_null() {
            *out = conv.output.as_ptr();
        }
        conv.output.len() as c_long
    }))
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Shared URL helper
// ---------------------------------------------------------------------------

/// Percent-encode a file-system path for use in a `file://` URL.
///
/// Keeps unreserved URI characters (RFC 3986 §2.3) plus `/`, `:`, `@`;
/// encodes everything else as `%XX`.  Matches the identical helper in
/// `wkhtmltoimage-cli` and `wkhtmltopdf-cli`.
fn percent_encode_path(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 16);
    for &byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'/'
            | b':'
            | b'@' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// ===========================================================================
// wkhtmltoimage exports
// ===========================================================================
//
// All exports follow the same pattern as the PDF side:
//   • opaque Boxes behind raw pointers
//   • catch_unwind on every export
//   • per-converter str_cache for borrowed `const char*`
//   • ownership transfer of global settings to the converter

// ---------------------------------------------------------------------------
// Opaque handle type aliases (image side)
// ---------------------------------------------------------------------------

/// Opaque type behind `wkhtmltoimage_global_settings *`.
pub type WkImageGlobalSettings = ImageGlobalSettings;

// ---------------------------------------------------------------------------
// C callback types (mirror the typedefs in image.h)
// ---------------------------------------------------------------------------

/// `void (*)(wkhtmltoimage_converter *, const char *)`
pub type WkhtmltoimageStrCallback =
    Option<unsafe extern "C" fn(*mut CImageConverter, *const c_char)>;
/// `void (*)(wkhtmltoimage_converter *, const int)`
pub type WkhtmltoimageIntCallback = Option<unsafe extern "C" fn(*mut CImageConverter, c_int)>;
/// `void (*)(wkhtmltoimage_converter *)`
pub type WkhtmltoimageVoidCallback = Option<unsafe extern "C" fn(*mut CImageConverter)>;

// ---------------------------------------------------------------------------
// Phase names for wkhtmltoimage
// ---------------------------------------------------------------------------

const IMAGE_PHASES: &[&str] = &["Loading page", "Rendering", "Done"];

// ---------------------------------------------------------------------------
// CImageConverter — state behind `wkhtmltoimage_converter *`
// ---------------------------------------------------------------------------

pub struct CImageConverter {
    /// Owned global settings (transferred from `create_global_settings`).
    global: ImageGlobalSettings,
    /// Inline HTML provided by `create_converter(settings, data)`, if any.
    inline_html: Option<String>,
    // Callbacks
    warning_cb: WkhtmltoimageStrCallback,
    error_cb: WkhtmltoimageStrCallback,
    phase_changed_cb: WkhtmltoimageVoidCallback,
    progress_changed_cb: WkhtmltoimageIntCallback,
    finished_cb: WkhtmltoimageIntCallback,
    // Output image bytes (borrowed by wkhtmltoimage_get_output until destroyed).
    output: Vec<u8>,
    http_error: i32,
    // Phase / progress tracking.
    cur_phase: i32,
    phase_count: i32,
    progress_str: String,
    /// Interned C strings — same lifetime trick as the PDF side.
    str_cache: std::collections::HashMap<String, std::ffi::CString>,
}

impl CImageConverter {
    fn new(global: ImageGlobalSettings, inline_html: Option<String>) -> Self {
        Self {
            global,
            inline_html,
            warning_cb: None,
            error_cb: None,
            phase_changed_cb: None,
            progress_changed_cb: None,
            finished_cb: None,
            output: Vec::new(),
            http_error: 0,
            cur_phase: -1,
            phase_count: IMAGE_PHASES.len() as i32,
            progress_str: String::new(),
            str_cache: std::collections::HashMap::new(),
        }
    }

    fn cache_str(&mut self, s: &str) -> *const c_char {
        self.str_cache
            .entry(s.to_owned())
            .or_insert_with(|| {
                let bytes: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
                // SAFETY: NUL bytes removed above.
                unsafe { std::ffi::CString::from_vec_unchecked(bytes) }
            })
            .as_ptr()
    }
}

// ---------------------------------------------------------------------------
// Internal callback helpers (image side)
// ---------------------------------------------------------------------------

unsafe fn img_phase_emit(conv: *mut CImageConverter, phase: i32) {
    (*conv).cur_phase = phase;
    let desc = IMAGE_PHASES.get(phase as usize).copied().unwrap_or("");
    (*conv).progress_str = desc.to_owned();
    if let Some(cb) = (*conv).phase_changed_cb {
        cb(conv);
    }
}

unsafe fn img_progress_emit(conv: *mut CImageConverter, pct: c_int) {
    let phase = (*conv).cur_phase;
    let desc = IMAGE_PHASES
        .get(phase.max(0) as usize)
        .copied()
        .unwrap_or("");
    (*conv).progress_str = format!("{desc} [{pct}%]");
    if let Some(cb) = (*conv).progress_changed_cb {
        cb(conv, pct);
    }
}

unsafe fn img_error_emit(conv: *mut CImageConverter, msg: &str) {
    if let Some(cb) = (*conv).error_cb {
        let bytes: Vec<u8> = msg.bytes().filter(|&b| b != 0).collect();
        // SAFETY: NUL bytes removed.
        let cs = std::ffi::CString::from_vec_unchecked(bytes);
        cb(conv, cs.as_ptr());
    }
}

unsafe fn img_finished_emit(conv: *mut CImageConverter, ok: c_int) {
    if let Some(cb) = (*conv).finished_cb {
        cb(conv, ok);
    }
}

// ---------------------------------------------------------------------------
// init / deinit / version (image)
// ---------------------------------------------------------------------------

/// `wkhtmltoimage_init(int use_graphics)` → 1.
#[no_mangle]
pub extern "C" fn wkhtmltoimage_init(_use_graphics: c_int) -> c_int {
    std::panic::catch_unwind(|| 1).unwrap_or(0)
}

/// `wkhtmltoimage_deinit()` → 1.
#[no_mangle]
pub extern "C" fn wkhtmltoimage_deinit() -> c_int {
    std::panic::catch_unwind(|| 1).unwrap_or(0)
}

/// `wkhtmltoimage_extended_qt()` → 1.
#[no_mangle]
pub extern "C" fn wkhtmltoimage_extended_qt() -> c_int {
    std::panic::catch_unwind(|| 1).unwrap_or(0)
}

/// `wkhtmltoimage_version()` → `const char*`.
#[no_mangle]
pub extern "C" fn wkhtmltoimage_version() -> *const c_char {
    std::panic::catch_unwind(|| VERSION_STR.as_ptr() as *const c_char).unwrap_or(empty_cstr())
}

// ---------------------------------------------------------------------------
// Global settings (image)
// ---------------------------------------------------------------------------

/// `wkhtmltoimage_create_global_settings()` → opaque handle.
#[no_mangle]
pub extern "C" fn wkhtmltoimage_create_global_settings() -> *mut WkImageGlobalSettings {
    std::panic::catch_unwind(|| Box::into_raw(Box::new(ImageGlobalSettings::default())))
        .unwrap_or(std::ptr::null_mut())
}

/// `wkhtmltoimage_destroy_global_settings(settings)`.
///
/// # Safety
/// `settings` must be a live pointer returned by
/// `wkhtmltoimage_create_global_settings` and not yet destroyed.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_destroy_global_settings(
    settings: *mut WkImageGlobalSettings,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !settings.is_null() {
            drop(Box::from_raw(settings));
        }
    }));
}

/// `wkhtmltoimage_set_global_setting(settings, name, value)` → 1 ok, 0 err.
///
/// # Safety
/// All pointer arguments must be valid (or null — null returns 0).
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_set_global_setting(
    settings: *mut WkImageGlobalSettings,
    name: *const c_char,
    value: *const c_char,
) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if settings.is_null() || name.is_null() || value.is_null() {
            return 0;
        }
        let g = &mut *settings;
        let name_s = std::ffi::CStr::from_ptr(name).to_str().unwrap_or("");
        let value_s = std::ffi::CStr::from_ptr(value).to_str().unwrap_or("");
        match set_image_global(g, name_s, value_s) {
            Ok(()) => 1,
            Err(_) => 0,
        }
    }))
    .unwrap_or(0)
}

/// `wkhtmltoimage_get_global_setting(settings, name, value, vs)` → 1 known, 0 unknown.
///
/// # Safety
/// `settings` and `name` must be valid.  `value` may be null.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_get_global_setting(
    settings: *mut WkImageGlobalSettings,
    name: *const c_char,
    value: *mut c_char,
    vs: c_int,
) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if settings.is_null() || name.is_null() {
            return 0;
        }
        let g = &*settings;
        let name_s = std::ffi::CStr::from_ptr(name).to_str().unwrap_or("");
        match get_image_global(g, name_s) {
            Some(s) => write_to_buf(&s, value, vs),
            None => 0,
        }
    }))
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Converter lifecycle (image)
// ---------------------------------------------------------------------------

/// `wkhtmltoimage_create_converter(settings, data)` → converter handle.
///
/// # Ownership
/// **Transfers** ownership of `settings` to the returned converter.
///
/// `data` (may be null) is optional inline HTML.  When non-null and non-empty
/// it is used as the HTML source; otherwise the converter uses the `"in"`
/// setting.
///
/// # Safety
/// `settings` must be a live pointer returned by
/// `wkhtmltoimage_create_global_settings`, or null.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_create_converter(
    settings: *mut WkImageGlobalSettings,
    data: *const c_char,
) -> *mut CImageConverter {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if settings.is_null() {
            return std::ptr::null_mut();
        }
        let gs = *Box::from_raw(settings);
        let inline_html: Option<String> = if data.is_null() {
            None
        } else {
            let s = std::ffi::CStr::from_ptr(data)
                .to_string_lossy()
                .into_owned();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        };
        Box::into_raw(Box::new(CImageConverter::new(gs, inline_html)))
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// `wkhtmltoimage_destroy_converter(converter)`.
///
/// # Safety
/// `converter` must be a live pointer not yet destroyed, or null.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_destroy_converter(converter: *mut CImageConverter) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            drop(Box::from_raw(converter));
        }
    }));
}

// ---------------------------------------------------------------------------
// Callback setters (image)
// ---------------------------------------------------------------------------

/// `wkhtmltoimage_set_warning_callback`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_set_warning_callback(
    converter: *mut CImageConverter,
    cb: WkhtmltoimageStrCallback,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            (*converter).warning_cb = cb;
        }
    }));
}

/// `wkhtmltoimage_set_error_callback`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_set_error_callback(
    converter: *mut CImageConverter,
    cb: WkhtmltoimageStrCallback,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            (*converter).error_cb = cb;
        }
    }));
}

/// `wkhtmltoimage_set_phase_changed_callback`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_set_phase_changed_callback(
    converter: *mut CImageConverter,
    cb: WkhtmltoimageVoidCallback,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            (*converter).phase_changed_cb = cb;
        }
    }));
}

/// `wkhtmltoimage_set_progress_changed_callback`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_set_progress_changed_callback(
    converter: *mut CImageConverter,
    cb: WkhtmltoimageIntCallback,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            (*converter).progress_changed_cb = cb;
        }
    }));
}

/// `wkhtmltoimage_set_finished_callback`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_set_finished_callback(
    converter: *mut CImageConverter,
    cb: WkhtmltoimageIntCallback,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !converter.is_null() {
            (*converter).finished_cb = cb;
        }
    }));
}

// ---------------------------------------------------------------------------
// convert — image pipeline
// ---------------------------------------------------------------------------

/// `wkhtmltoimage_convert(converter)` → 1 on success, 0 on failure.
///
/// Pipeline:
///   1. Resolve source: inline HTML > `in` setting (file → `file://` URL or URL).
///   2. Spawn `ChromiumRenderer`.
///   3. `open` the source → `wait_ready` → `snapshot` → `image::produce`.
///   4. Store bytes in `converter.output`.
///   5. Drive phase/progress/finished callbacks.
///
/// # Safety
/// `converter` must be a live pointer or null.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_convert(converter: *mut CImageConverter) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> c_int {
        if converter.is_null() {
            return 0;
        }

        // Resolve the source.
        let source: Source = {
            let conv = &*converter;
            if let Some(html) = &conv.inline_html {
                Source::Html(html.clone())
            } else if let Some(path) = &conv.global.in_path {
                if path == "-" {
                    // Stdin deferral — emit error; full stdin support is a T3 concern.
                    img_error_emit(converter, "stdin input ('-') is not yet supported via the C ABI; set 'in' to a file path or URL");
                    (*converter).http_error = 1;
                    img_finished_emit(converter, 0);
                    return 0;
                } else if path.starts_with("http://") || path.starts_with("https://") || path.starts_with("file://") {
                    Source::Url(path.clone())
                } else {
                    // Treat as a local file path → file:// URL (percent-encoded).
                    let abs_path = std::path::Path::new(path);
                    let file_url = if abs_path.is_absolute() {
                        format!("file://{}", percent_encode_path(abs_path))
                    } else {
                        match std::env::current_dir() {
                            Ok(cwd) => format!("file://{}", percent_encode_path(&cwd.join(path))),
                            Err(_) => format!("file://{}", percent_encode_path(abs_path)),
                        }
                    };
                    Source::Url(file_url)
                }
            } else {
                img_error_emit(converter, "no input source: set the 'in' setting or pass data to create_converter");
                (*converter).http_error = 1;
                img_finished_emit(converter, 0);
                return 0;
            }
        };

        let (load_settings, img_opts, proxy) = {
            let conv = &*converter;
            (
                conv.global.to_load_settings(),
                conv.global.to_image_opts(),
                conv.global.proxy.clone(),
            )
        };

        // Phase 0: Loading page
        img_phase_emit(converter, 0);
        img_progress_emit(converter, 0);

        let mut renderer = match wkhtmltox_render_chromium::renderer::ChromiumRenderer::spawn_opts(
            wkhtmltox_render_chromium::renderer::SpawnOpts { proxy },
        ) {
            Ok(r) => r,
            Err(e) => {
                img_error_emit(converter, &format!("renderer spawn failed: {e}"));
                (*converter).http_error = -1;
                img_finished_emit(converter, 0);
                return 0;
            }
        };

        img_progress_emit(converter, 20);

        let page = match renderer.open(&source, &load_settings) {
            Ok(p) => p,
            Err(e) => {
                img_error_emit(converter, &format!("open failed: {e}"));
                (*converter).http_error = 1;
                img_finished_emit(converter, 0);
                return 0;
            }
        };

        let jsdelay = { (*converter).global.javascript_delay_ms };
        let ready_policy = ReadyPolicy {
            javascript_delay_ms: jsdelay,
            window_status: None,
        };
        if let Err(e) = renderer.wait_ready(page, &ready_policy) {
            img_error_emit(converter, &format!("wait_ready failed: {e}"));
            (*converter).http_error = 1;
            img_finished_emit(converter, 0);
            return 0;
        }

        img_progress_emit(converter, 50);

        // Phase 1: Rendering
        img_phase_emit(converter, 1);

        let snap_fmt = img_opts.format;
        let snap_quality = img_opts.quality;
        let snap_opts = SnapshotOpts {
            format: snap_fmt,
            crop: img_opts.crop,
            scale: img_opts.zoom,
            quality: snap_quality,
        };

        let raw = match renderer.snapshot(page, &snap_opts) {
            Ok(r) => r,
            Err(e) => {
                img_error_emit(converter, &format!("snapshot failed: {e}"));
                (*converter).http_error = 1;
                img_finished_emit(converter, 0);
                return 0;
            }
        };

        img_progress_emit(converter, 80);

        // CDP already applied the crop via the clip parameter in SnapshotOpts;
        // produce must not re-crop the already-cropped image.
        let mut produce_opts = img_opts;
        produce_opts.crop = None;
        let bytes = match image_produce(&raw, &produce_opts) {
            Ok(b) => b,
            Err(e) => {
                img_error_emit(converter, &format!("image produce failed: {e}"));
                (*converter).http_error = 1;
                img_finished_emit(converter, 0);
                return 0;
            }
        };

        (*converter).output = bytes;

        // Phase 2: Done
        img_phase_emit(converter, 2);
        img_progress_emit(converter, 100);
        img_finished_emit(converter, 1);
        1
    }))
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Phase / progress accessors (image)
// ---------------------------------------------------------------------------

/// `wkhtmltoimage_current_phase(converter)` → current phase index, or -1.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_current_phase(converter: *mut CImageConverter) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            return -1;
        }
        (*converter).cur_phase
    }))
    .unwrap_or(-1)
}

/// `wkhtmltoimage_phase_count(converter)` → number of phases.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_phase_count(converter: *mut CImageConverter) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            return 0;
        }
        (*converter).phase_count
    }))
    .unwrap_or(0)
}

/// `wkhtmltoimage_phase_description(converter, phase)` → `const char*`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_phase_description(
    converter: *mut CImageConverter,
    phase: c_int,
) -> *const c_char {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            return empty_cstr();
        }
        let name = IMAGE_PHASES.get(phase as usize).copied().unwrap_or("");
        (*converter).cache_str(name)
    }))
    .unwrap_or(empty_cstr())
}

/// `wkhtmltoimage_progress_string(converter)` → `const char*`.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_progress_string(
    converter: *mut CImageConverter,
) -> *const c_char {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            return empty_cstr();
        }
        let s = (*converter).progress_str.clone();
        (*converter).cache_str(&s)
    }))
    .unwrap_or(empty_cstr())
}

/// `wkhtmltoimage_http_error_code(converter)` → HTTP error code (0 = none).
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_http_error_code(converter: *mut CImageConverter) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            return 0;
        }
        (*converter).http_error
    }))
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Output (image)
// ---------------------------------------------------------------------------

/// `wkhtmltoimage_get_output(converter, out)` → output length in bytes.
///
/// Sets `*out` to point at the image bytes stored inside the converter.
/// The pointer is valid until the converter is destroyed or `convert` is
/// called again.
///
/// # Safety
/// `converter` must be live or null.  `out` may be null.
#[no_mangle]
pub unsafe extern "C" fn wkhtmltoimage_get_output(
    converter: *mut CImageConverter,
    out: *mut *const c_uchar,
) -> c_long {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if converter.is_null() {
            if !out.is_null() {
                *out = std::ptr::null();
            }
            return 0;
        }
        let conv = &*converter;
        if !out.is_null() {
            *out = conv.output.as_ptr();
        }
        conv.output.len() as c_long
    }))
    .unwrap_or(0)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Null-pointer safety — none of these may crash or panic.
    // -----------------------------------------------------------------------

    #[test]
    fn null_init_deinit() {
        // These take no pointers; just confirm they return 1.
        assert_eq!(wkhtmltopdf_init(0), 1);
        assert_eq!(wkhtmltopdf_deinit(), 1);
        assert_eq!(wkhtmltopdf_extended_qt(), 1);
    }

    #[test]
    fn null_set_global_setting_returns_zero() {
        unsafe {
            // All-null → 0
            assert_eq!(
                wkhtmltopdf_set_global_setting(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                ),
                0
            );
        }
    }

    #[test]
    fn null_get_global_setting_returns_zero() {
        unsafe {
            assert_eq!(
                wkhtmltopdf_get_global_setting(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    0,
                ),
                0
            );
        }
    }

    #[test]
    fn null_create_converter_returns_null() {
        unsafe {
            let c = wkhtmltopdf_create_converter(std::ptr::null_mut());
            assert!(c.is_null());
        }
    }

    #[test]
    fn null_destroy_converter_is_noop() {
        unsafe {
            wkhtmltopdf_destroy_converter(std::ptr::null_mut());
        }
    }

    #[test]
    fn null_phase_accessors_return_safe_defaults() {
        unsafe {
            assert_eq!(wkhtmltopdf_current_phase(std::ptr::null_mut()), -1);
            assert_eq!(wkhtmltopdf_phase_count(std::ptr::null_mut()), 0);
            let p = wkhtmltopdf_phase_description(std::ptr::null_mut(), 0);
            assert!(!p.is_null()); // must return empty CStr, not null
            let p2 = wkhtmltopdf_progress_string(std::ptr::null_mut());
            assert!(!p2.is_null());
        }
    }

    #[test]
    fn null_get_output_returns_zero_length() {
        unsafe {
            let mut ptr: *const c_uchar = std::ptr::null();
            let len = wkhtmltopdf_get_output(std::ptr::null_mut(), &mut ptr);
            assert_eq!(len, 0);
            assert!(ptr.is_null());
        }
    }

    // -----------------------------------------------------------------------
    // Settings get / set round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn global_settings_set_get_page_size() {
        unsafe {
            let gs = wkhtmltopdf_create_global_settings();
            assert!(!gs.is_null());

            let name = CString::new("size.pageSize").unwrap();
            let value = CString::new("Letter").unwrap();
            let r = wkhtmltopdf_set_global_setting(gs, name.as_ptr(), value.as_ptr());
            assert_eq!(r, 1, "set should return 1");

            let mut buf = [0u8; 64];
            let r2 = wkhtmltopdf_get_global_setting(
                gs,
                name.as_ptr(),
                buf.as_mut_ptr().cast::<c_char>(),
                64,
            );
            assert_eq!(r2, 1, "get should return 1");
            let got = CStr::from_ptr(buf.as_ptr().cast::<c_char>())
                .to_str()
                .unwrap();
            assert_eq!(got, "Letter");

            wkhtmltopdf_destroy_global_settings(gs);
        }
    }

    #[test]
    fn global_settings_unknown_name_returns_zero() {
        unsafe {
            let gs = wkhtmltopdf_create_global_settings();
            let name = CString::new("totally.unknown.name").unwrap();
            let value = CString::new("x").unwrap();
            let r = wkhtmltopdf_set_global_setting(gs, name.as_ptr(), value.as_ptr());
            assert_eq!(r, 0);
            let mut buf = [0u8; 32];
            let r2 = wkhtmltopdf_get_global_setting(
                gs,
                name.as_ptr(),
                buf.as_mut_ptr().cast::<c_char>(),
                32,
            );
            assert_eq!(r2, 0);
            wkhtmltopdf_destroy_global_settings(gs);
        }
    }

    #[test]
    fn object_settings_set_get_page() {
        unsafe {
            let os = wkhtmltopdf_create_object_settings();
            assert!(!os.is_null());

            let name = CString::new("page").unwrap();
            let value = CString::new("https://example.com").unwrap();
            let r = wkhtmltopdf_set_object_setting(os, name.as_ptr(), value.as_ptr());
            assert_eq!(r, 1);

            let mut buf = [0u8; 128];
            let r2 = wkhtmltopdf_get_object_setting(
                os,
                name.as_ptr(),
                buf.as_mut_ptr().cast::<c_char>(),
                128,
            );
            assert_eq!(r2, 1);
            let got = CStr::from_ptr(buf.as_ptr().cast::<c_char>())
                .to_str()
                .unwrap();
            assert_eq!(got, "https://example.com");

            wkhtmltopdf_destroy_object_settings(os);
        }
    }

    #[test]
    fn version_string_is_not_null_and_not_empty() {
        let ptr = wkhtmltopdf_version();
        assert!(!ptr.is_null());
        unsafe {
            let s = CStr::from_ptr(ptr).to_str().unwrap();
            assert!(!s.is_empty(), "version string must not be empty");
        }
    }

    #[test]
    fn phase_count_matches_phases_array() {
        unsafe {
            let gs = wkhtmltopdf_create_global_settings();
            // gs ownership transferred to the converter here.
            let conv = wkhtmltopdf_create_converter(gs);
            assert!(!conv.is_null());
            let pc = wkhtmltopdf_phase_count(conv);
            assert_eq!(pc as usize, PHASES.len());
            // destroy_converter also drops the owned GlobalSettings.
            wkhtmltopdf_destroy_converter(conv);
            // Do NOT call wkhtmltopdf_destroy_global_settings(gs) — double-free.
        }
    }

    #[test]
    fn phase_description_returns_known_string() {
        unsafe {
            let gs = wkhtmltopdf_create_global_settings();
            // gs ownership transferred to the converter here.
            let conv = wkhtmltopdf_create_converter(gs);
            let desc = wkhtmltopdf_phase_description(conv, 0);
            assert!(!desc.is_null());
            let s = CStr::from_ptr(desc).to_str().unwrap();
            assert_eq!(s, PHASES[0]);
            wkhtmltopdf_destroy_converter(conv);
            // Do NOT call wkhtmltopdf_destroy_global_settings(gs) — double-free.
        }
    }

    #[test]
    fn phase_description_out_of_range_returns_empty() {
        unsafe {
            let gs = wkhtmltopdf_create_global_settings();
            // gs ownership transferred to the converter here.
            let conv = wkhtmltopdf_create_converter(gs);
            let desc = wkhtmltopdf_phase_description(conv, 999);
            assert!(!desc.is_null());
            let s = CStr::from_ptr(desc).to_str().unwrap();
            assert_eq!(s, "");
            wkhtmltopdf_destroy_converter(conv);
            // Do NOT call wkhtmltopdf_destroy_global_settings(gs) — double-free.
        }
    }

    #[test]
    fn get_output_before_convert_is_zero_length() {
        unsafe {
            let gs = wkhtmltopdf_create_global_settings();
            // gs ownership transferred to the converter here.
            let conv = wkhtmltopdf_create_converter(gs);
            let mut ptr: *const c_uchar = std::ptr::null();
            let len = wkhtmltopdf_get_output(conv, &mut ptr);
            assert_eq!(len, 0);
            // ptr may be non-null (pointing at an empty Vec's ptr()) — that's OK.
            wkhtmltopdf_destroy_converter(conv);
            // Do NOT call wkhtmltopdf_destroy_global_settings(gs) — double-free.
        }
    }

    #[test]
    fn add_object_and_callback_setters_dont_panic() {
        unsafe {
            let gs = wkhtmltopdf_create_global_settings();
            let os = wkhtmltopdf_create_object_settings();
            // gs ownership transferred to the converter here.
            let conv = wkhtmltopdf_create_converter(gs);

            wkhtmltopdf_set_warning_callback(conv, None);
            wkhtmltopdf_set_error_callback(conv, None);
            wkhtmltopdf_set_phase_changed_callback(conv, None);
            wkhtmltopdf_set_progress_changed_callback(conv, None);
            wkhtmltopdf_set_finished_callback(conv, None);

            let data = CString::new("<h1>test</h1>").unwrap();
            // os ownership transferred to the converter here.
            wkhtmltopdf_add_object(conv, os, data.as_ptr());

            // destroy_converter drops the converter, its owned GlobalSettings,
            // and all owned PdfObjectSettings.
            wkhtmltopdf_destroy_converter(conv);
            // Do NOT call destroy_object_settings(os) or
            // destroy_global_settings(gs) — both would be double-frees.
        }
    }

    // -----------------------------------------------------------------------
    // Ownership transfer tests — verify no double-free path.
    // -----------------------------------------------------------------------

    /// After passing `gs` to `create_converter`, the caller must NOT destroy it.
    /// `destroy_converter` frees the converter AND its owned `GlobalSettings`.
    #[test]
    fn create_converter_takes_ownership_no_double_free() {
        unsafe {
            let gs = wkhtmltopdf_create_global_settings();
            assert!(!gs.is_null());

            // Ownership transferred — do not call destroy_global_settings.
            let conv = wkhtmltopdf_create_converter(gs);
            assert!(!conv.is_null());

            // This frees the converter AND the owned GlobalSettings.
            wkhtmltopdf_destroy_converter(conv);
            // Calling destroy_global_settings(gs) here would be a double-free.
        }
    }

    /// After passing `os` to `add_object`, the caller must NOT destroy it.
    /// `destroy_converter` frees all owned `PdfObjectSettings` too.
    #[test]
    fn add_object_takes_ownership_no_double_free() {
        unsafe {
            let gs = wkhtmltopdf_create_global_settings();
            assert!(!gs.is_null());
            let os = wkhtmltopdf_create_object_settings();
            assert!(!os.is_null());

            // gs ownership transferred.
            let conv = wkhtmltopdf_create_converter(gs);
            assert!(!conv.is_null());

            let data = CString::new("<p>ownership test</p>").unwrap();
            // os ownership transferred.
            wkhtmltopdf_add_object(conv, os, data.as_ptr());

            // Frees converter + owned GlobalSettings + owned PdfObjectSettings.
            wkhtmltopdf_destroy_converter(conv);
            // Calling destroy_object_settings(os) or destroy_global_settings(gs)
            // here would be double-frees.
        }
    }

    // -----------------------------------------------------------------------
    // Chrome-dependent integration test — run with: cargo test --ignored
    // -----------------------------------------------------------------------

    /// Full convert pipeline: inline HTML `<h1>Hi</h1>` → PDF starting with `%PDF`.
    ///
    /// Requires Chromium to be installed and discoverable.  Excluded from the
    /// default test run; use `cargo test -p wkhtmltox-capi -- --ignored` to run.
    #[test]
    #[ignore = "requires Chrome; run explicitly with --ignored"]
    fn convert_inline_html_produces_pdf() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static FINISHED_OK: AtomicBool = AtomicBool::new(false);

        unsafe extern "C" fn on_finished(_conv: *mut CConverter, ok: c_int) {
            FINISHED_OK.store(ok != 0, Ordering::Relaxed);
        }

        unsafe {
            let gs = wkhtmltopdf_create_global_settings();
            assert!(!gs.is_null());

            // Set A4 page size.
            let k = CString::new("size.pageSize").unwrap();
            let v = CString::new("A4").unwrap();
            assert_eq!(
                wkhtmltopdf_set_global_setting(gs, k.as_ptr(), v.as_ptr()),
                1
            );

            let os = wkhtmltopdf_create_object_settings();
            assert!(!os.is_null());

            let conv = wkhtmltopdf_create_converter(gs);
            assert!(!conv.is_null());

            // Register finished callback.
            wkhtmltopdf_set_finished_callback(conv, Some(on_finished));

            // Add inline HTML object.
            let html = CString::new("<html><body><h1>Hi</h1></body></html>").unwrap();
            wkhtmltopdf_add_object(conv, os, html.as_ptr());

            // Convert.
            let ok = wkhtmltopdf_convert(conv);
            assert_eq!(ok, 1, "convert should return 1");
            assert!(
                FINISHED_OK.load(Ordering::Relaxed),
                "finished callback should have ok=1"
            );

            // Check output starts with %PDF.
            let mut out_ptr: *const c_uchar = std::ptr::null();
            let len = wkhtmltopdf_get_output(conv, &mut out_ptr);
            assert!(len >= 4, "output must be at least 4 bytes");
            assert!(!out_ptr.is_null());
            let header = std::slice::from_raw_parts(out_ptr, 4.min(len as usize));
            assert_eq!(header, b"%PDF", "output must start with %PDF");

            // destroy_converter also frees the owned GlobalSettings (gs) and
            // PdfObjectSettings (os) — do NOT call their individual destroy
            // functions, which would be double-frees.
            wkhtmltopdf_destroy_converter(conv);
        }
    }

    // -----------------------------------------------------------------------
    // wkhtmltoimage null-pointer safety
    // -----------------------------------------------------------------------

    #[test]
    fn image_null_init_deinit() {
        assert_eq!(wkhtmltoimage_init(0), 1);
        assert_eq!(wkhtmltoimage_deinit(), 1);
        assert_eq!(wkhtmltoimage_extended_qt(), 1);
    }

    #[test]
    fn image_null_set_global_returns_zero() {
        unsafe {
            assert_eq!(
                wkhtmltoimage_set_global_setting(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                ),
                0
            );
        }
    }

    #[test]
    fn image_null_get_global_returns_zero() {
        unsafe {
            assert_eq!(
                wkhtmltoimage_get_global_setting(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    0,
                ),
                0
            );
        }
    }

    #[test]
    fn image_null_create_converter_returns_null() {
        unsafe {
            let c = wkhtmltoimage_create_converter(std::ptr::null_mut(), std::ptr::null());
            assert!(c.is_null());
        }
    }

    #[test]
    fn image_null_destroy_converter_is_noop() {
        unsafe {
            wkhtmltoimage_destroy_converter(std::ptr::null_mut());
        }
    }

    #[test]
    fn image_null_phase_accessors_return_safe_defaults() {
        unsafe {
            assert_eq!(wkhtmltoimage_current_phase(std::ptr::null_mut()), -1);
            assert_eq!(wkhtmltoimage_phase_count(std::ptr::null_mut()), 0);
            let p = wkhtmltoimage_phase_description(std::ptr::null_mut(), 0);
            assert!(!p.is_null());
            let p2 = wkhtmltoimage_progress_string(std::ptr::null_mut());
            assert!(!p2.is_null());
        }
    }

    #[test]
    fn image_null_get_output_returns_zero_length() {
        unsafe {
            let mut ptr: *const c_uchar = std::ptr::null();
            let len = wkhtmltoimage_get_output(std::ptr::null_mut(), &mut ptr);
            assert_eq!(len, 0);
            assert!(ptr.is_null());
        }
    }

    // -----------------------------------------------------------------------
    // wkhtmltoimage settings get/set round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn image_settings_set_get_fmt() {
        use std::ffi::CString;
        unsafe {
            let gs = wkhtmltoimage_create_global_settings();
            assert!(!gs.is_null());

            let name = CString::new("fmt").unwrap();
            let val = CString::new("jpeg").unwrap();
            assert_eq!(
                wkhtmltoimage_set_global_setting(gs, name.as_ptr(), val.as_ptr()),
                1
            );

            let mut buf = [0u8; 32];
            let r = wkhtmltoimage_get_global_setting(
                gs,
                name.as_ptr(),
                buf.as_mut_ptr().cast::<c_char>(),
                32,
            );
            assert_eq!(r, 1);
            let got = std::ffi::CStr::from_ptr(buf.as_ptr().cast::<c_char>())
                .to_str()
                .unwrap();
            assert_eq!(got, "jpeg");

            wkhtmltoimage_destroy_global_settings(gs);
        }
    }

    #[test]
    fn image_settings_unknown_name_returns_zero() {
        use std::ffi::CString;
        unsafe {
            let gs = wkhtmltoimage_create_global_settings();
            let name = CString::new("totally.unknown.xyz").unwrap();
            let val = CString::new("x").unwrap();
            let r = wkhtmltoimage_set_global_setting(gs, name.as_ptr(), val.as_ptr());
            assert_eq!(r, 0);
            let mut buf = [0u8; 32];
            let r2 = wkhtmltoimage_get_global_setting(
                gs,
                name.as_ptr(),
                buf.as_mut_ptr().cast::<c_char>(),
                32,
            );
            assert_eq!(r2, 0);
            wkhtmltoimage_destroy_global_settings(gs);
        }
    }

    #[test]
    fn image_version_not_null_or_empty() {
        let ptr = wkhtmltoimage_version();
        assert!(!ptr.is_null());
        unsafe {
            let s = std::ffi::CStr::from_ptr(ptr).to_str().unwrap();
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn image_phase_count_matches_array() {
        unsafe {
            let gs = wkhtmltoimage_create_global_settings();
            let conv = wkhtmltoimage_create_converter(gs, std::ptr::null());
            assert!(!conv.is_null());
            let pc = wkhtmltoimage_phase_count(conv);
            assert_eq!(pc as usize, IMAGE_PHASES.len());
            wkhtmltoimage_destroy_converter(conv);
        }
    }

    #[test]
    fn image_create_converter_takes_ownership() {
        unsafe {
            let gs = wkhtmltoimage_create_global_settings();
            assert!(!gs.is_null());
            let conv = wkhtmltoimage_create_converter(gs, std::ptr::null());
            assert!(!conv.is_null());
            // destroy_converter frees the converter AND the owned settings.
            wkhtmltoimage_destroy_converter(conv);
        }
    }

    #[test]
    fn image_get_output_before_convert_is_empty() {
        unsafe {
            let gs = wkhtmltoimage_create_global_settings();
            let conv = wkhtmltoimage_create_converter(gs, std::ptr::null());
            let mut ptr: *const c_uchar = std::ptr::null();
            let len = wkhtmltoimage_get_output(conv, &mut ptr);
            assert_eq!(len, 0);
            wkhtmltoimage_destroy_converter(conv);
        }
    }

    // -----------------------------------------------------------------------
    // Chrome-dependent integration test — run with: cargo test --ignored
    // -----------------------------------------------------------------------

    /// Full image pipeline: inline HTML `<h1>Hi</h1>` → PNG starting with `\x89PNG`.
    ///
    /// Requires Chromium.  Run with `cargo test -p wkhtmltox-capi -- --ignored`.
    #[test]
    #[ignore = "requires Chrome; run explicitly with --ignored"]
    fn convert_inline_html_produces_png() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static IMG_FINISHED_OK: AtomicBool = AtomicBool::new(false);

        unsafe extern "C" fn on_img_finished(_conv: *mut CImageConverter, ok: c_int) {
            IMG_FINISHED_OK.store(ok != 0, Ordering::Relaxed);
        }

        unsafe {
            wkhtmltoimage_init(0);

            let gs = wkhtmltoimage_create_global_settings();
            assert!(!gs.is_null());

            // Set PNG format.
            let k_fmt = std::ffi::CString::new("fmt").unwrap();
            let v_fmt = std::ffi::CString::new("png").unwrap();
            assert_eq!(
                wkhtmltoimage_set_global_setting(gs, k_fmt.as_ptr(), v_fmt.as_ptr()),
                1
            );

            // Inline HTML data.
            let data = std::ffi::CString::new("<html><body><h1>Hi</h1></body></html>").unwrap();
            let conv = wkhtmltoimage_create_converter(gs, data.as_ptr());
            assert!(!conv.is_null());

            wkhtmltoimage_set_finished_callback(conv, Some(on_img_finished));

            let ok = wkhtmltoimage_convert(conv);
            assert_eq!(ok, 1, "convert should return 1");
            assert!(
                IMG_FINISHED_OK.load(Ordering::Relaxed),
                "finished callback should have ok=1"
            );

            // Check output starts with PNG magic bytes.
            let mut out_ptr: *const c_uchar = std::ptr::null();
            let len = wkhtmltoimage_get_output(conv, &mut out_ptr);
            assert!(len >= 4, "output must be at least 4 bytes, got {len}");
            assert!(!out_ptr.is_null());
            let header = std::slice::from_raw_parts(out_ptr, 4.min(len as usize));
            assert_eq!(header, b"\x89PNG", "output must start with PNG magic bytes");

            wkhtmltoimage_destroy_converter(conv);
            wkhtmltoimage_deinit();
        }
    }
}
