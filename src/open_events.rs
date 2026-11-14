//! Files and folders the operating system asks Slate to open while it is
//! starting or already running: Finder's "Open With", a double-click on a
//! file Slate is the default app for, something dropped on the Dock icon.
//!
//! The request arrives on the main thread from outside the UI code, so it
//! is parked in a queue and the app collects it on its next frame.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use eframe::egui::Context;

static QUEUE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static WAKE: OnceLock<Context> = OnceLock::new();

/// Once the UI exists, let incoming requests wake it up.
pub fn set_waker(ctx: &Context) {
    let _ = WAKE.set(ctx.clone());
}

pub fn push(path: PathBuf) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.push(path);
    }
    if let Some(ctx) = WAKE.get() {
        ctx.request_repaint();
    }
}

/// Everything asked for since the last call.
pub fn take() -> Vec<PathBuf> {
    QUEUE.lock().map(|mut queue| std::mem::take(&mut *queue)).unwrap_or_default()
}

/// Called from Objective-C (`macos_open.m`) with a NUL-terminated path.
#[cfg(unix)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
extern "C" fn opened(path: *const std::ffi::c_char) {
    use std::os::unix::ffi::OsStrExt;
    if path.is_null() {
        return;
    }
    // SAFETY: AppKit hands us a valid C string that lives for this call.
    let bytes = unsafe { std::ffi::CStr::from_ptr(path) }.to_bytes();
    push(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)));
}

#[cfg(target_os = "macos")]
extern "C" {
    fn slate_install_open_handler(callback: extern "C" fn(*const std::ffi::c_char));
}

/// Start listening. Must run before the event loop starts.
pub fn install() {
    #[cfg(target_os = "macos")]
    // SAFETY: plain C call; the callback is a static function.
    unsafe {
        slate_install_open_handler(opened);
    }
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn callback_queues_the_path() {
        super::opened(c"/tmp/some file.rs".as_ptr());
        super::opened(std::ptr::null());
        assert_eq!(super::take(), vec![std::path::PathBuf::from("/tmp/some file.rs")]);
        assert!(super::take().is_empty());
    }
}
