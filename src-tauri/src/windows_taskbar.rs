//! The Windows taskbar asks for ICON_BIG, whereas Tauri 2.12's set_icon sets only ICON_SMALL.
//! Tao initially clears ICON_BIG when no separate platform taskbar icon is configured.
//! Keep this Windows-only FFI boundary private; no arbitrary handles cross the public API.
use std::ffi::c_void;
use tauri::{Runtime, WebviewWindow};

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}
#[link(name = "user32")]
extern "system" {
    fn LoadImageW(instance: *mut c_void, name: *const u16, kind: u32, width: i32, height: i32, flags: u32) -> *mut c_void;
    fn SendMessageW(window: *mut c_void, message: u32, wparam: usize, lparam: isize) -> isize;
}

pub fn set_taskbar_icon<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    let hwnd = window.hwnd()?.0;
    // SAFETY: NULL selects this process's executable. Resource 32512 is the icon ID embedded by
    // tauri-build. MAKEINTRESOURCE is a low-valued pointer interpreted as an integer by LoadImageW,
    // never dereferenced by Rust. IMAGE_ICON=1; LR_SHARED=0x8000 keeps the handle owned by Windows
    // for the module's lifetime, so it must NOT be destroyed or dropped after WM_SETICON.
    let icon = unsafe { LoadImageW(GetModuleHandleW(std::ptr::null()), 32512usize as *const u16, 1, 64, 64, 0x8000) };
    if icon.is_null() { return Err(std::io::Error::last_os_error().into()); }
    // SAFETY: Tauri supplied a live HWND; setup runs on its owning UI thread. Both messages only
    // copy/return an HICON value, and the shared icon outlives the window. No borrowed old icon is freed.
    unsafe { SendMessageW(hwnd, 0x80, 1, icon as isize); } // WM_SETICON / ICON_BIG
    let actual = unsafe { SendMessageW(hwnd, 0x7F, 1, 0) }; // WM_GETICON / ICON_BIG
    if actual != icon as isize {
        return Err(std::io::Error::other("Windows did not retain the taskbar icon").into());
    }
    Ok(())
}
