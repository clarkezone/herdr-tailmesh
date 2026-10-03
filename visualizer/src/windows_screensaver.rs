use std::{cell::Cell, num::NonZeroIsize, ptr, rc::Rc, sync::Arc};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    System::RemoteDesktop::{
        NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
    },
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetClientRect, GetCursorPos, GetForegroundWindow, IsWindow, MB_ICONINFORMATION, MB_OK,
            MessageBoxW, PBT_APMSUSPEND, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, WM_NCDESTROY,
            WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK,
        },
    },
};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    platform::windows::WindowAttributesExtWindows,
    raw_window_handle::{RawWindowHandle, Win32WindowHandle},
    window::{Window, WindowAttributes},
};

fn hwnd(handle: usize) -> HWND {
    handle as HWND
}

pub fn should_exit_for_message(message: u32, parameter: usize) -> bool {
    message == WM_POWERBROADCAST && parameter == PBT_APMSUSPEND as usize
        || message == WM_WTSSESSION_CHANGE && parameter == WTS_SESSION_LOCK as usize
}

fn window_hwnd(window: &Window) -> Result<HWND, String> {
    use winit::raw_window_handle::HasWindowHandle;
    let RawWindowHandle::Win32(handle) =
        window.window_handle().map_err(|e| e.to_string())?.as_raw()
    else {
        return Err("Expected a Win32 window handle".into());
    };
    Ok(handle.hwnd.get() as HWND)
}

pub fn owns_foreground<'a>(windows: impl Iterator<Item = &'a Window>) -> Result<bool, String> {
    let foreground = unsafe { GetForegroundWindow() };
    for window in windows {
        if window_hwnd(window)? == foreground {
            return Ok(true);
        }
    }
    Ok(false)
}

pub struct SessionNotifications {
    window: Arc<Window>,
    hook: Rc<ShutdownHook>,
}
struct ShutdownHook {
    requested: Rc<Cell<bool>>,
    attached: Cell<bool>,
}
const SUBCLASS_ID: usize = 0x484d5343;

unsafe extern "system" fn shutdown_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    // SetWindowSubclass owns one Rc until explicit removal or WM_NCDESTROY.
    let hook = unsafe { &*(data as *const ShutdownHook) };
    if should_exit_for_message(message, wparam) {
        hook.requested.set(true);
    }
    if message == WM_NCDESTROY && hook.attached.replace(false) {
        unsafe {
            RemoveWindowSubclass(window, Some(shutdown_proc), id);
            drop(Rc::from_raw(data as *const ShutdownHook));
        }
    }
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}

impl SessionNotifications {
    pub fn register(window: Arc<Window>, requested: Rc<Cell<bool>>) -> Result<Self, String> {
        let handle = window_hwnd(&window)?;
        if unsafe { WTSRegisterSessionNotification(handle, NOTIFY_FOR_THIS_SESSION) } == 0 {
            return Err(format!(
                "Cannot register screensaver session notifications: {}",
                std::io::Error::last_os_error()
            ));
        }
        let guard = Self {
            window,
            hook: Rc::new(ShutdownHook {
                requested,
                attached: Cell::new(false),
            }),
        };
        let native = Rc::into_raw(guard.hook.clone());
        if unsafe { SetWindowSubclass(handle, Some(shutdown_proc), SUBCLASS_ID, native as usize) }
            == 0
        {
            unsafe {
                drop(Rc::from_raw(native));
            }
            return Err("Cannot install screensaver shutdown window hook".into());
        }
        guard.hook.attached.set(true);
        Ok(guard)
    }
}
impl Drop for SessionNotifications {
    fn drop(&mut self) {
        match window_hwnd(&self.window) {
            Ok(handle) => {
                if self.hook.attached.get() {
                    if unsafe { RemoveWindowSubclass(handle, Some(shutdown_proc), SUBCLASS_ID) }
                        != 0
                    {
                        self.hook.attached.set(false);
                        unsafe {
                            drop(Rc::from_raw(Rc::as_ptr(&self.hook)));
                        }
                    } else {
                        // Keep the native Rc alive until WM_NCDESTROY if removal fails.
                        log::warn!("Cannot remove screensaver shutdown window hook");
                    }
                }
                if unsafe { IsWindow(handle) } != 0
                    && unsafe { WTSUnRegisterSessionNotification(handle) } == 0
                {
                    log::warn!(
                        "Cannot unregister screensaver session notifications: {}",
                        std::io::Error::last_os_error()
                    );
                }
            }
            Err(error) => {
                log::warn!("Cannot unregister screensaver session notifications: {error}")
            }
        }
    }
}

pub fn preview_size(parent: usize) -> Result<Option<PhysicalSize<u32>>, String> {
    let mut rect = RECT::default();
    // Windows owns this HWND; revalidate on each tick rather than retaining a reference.
    unsafe {
        if IsWindow(hwnd(parent)) == 0 {
            return Ok(None);
        }
        if GetClientRect(hwnd(parent), &mut rect) == 0 {
            return Err(format!(
                "Cannot read preview parent: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(Some(PhysicalSize::new(
        (rect.right - rect.left).max(0) as u32,
        (rect.bottom - rect.top).max(0) as u32,
    )))
}

pub fn preview_attributes(parent: usize) -> Result<WindowAttributes, String> {
    let size = preview_size(parent)?.ok_or("Preview parent HWND is not a live window")?;
    let parent = NonZeroIsize::new(parent as isize).ok_or("Preview HWND must be nonzero")?;
    let handle = RawWindowHandle::Win32(Win32WindowHandle::new(parent));
    // The parent is validated above and remains owned by the Windows settings host.
    Ok(
        unsafe { WindowAttributes::default().with_parent_window(Some(handle)) }
            .with_title("Herdr mesh screensaver preview")
            .with_decorations(false)
            .with_skip_taskbar(true)
            .with_active(false)
            .with_position(PhysicalPosition::new(0, 0))
            .with_inner_size(size),
    )
}

pub fn sync_preview(window: &Window, parent: usize) -> Result<bool, String> {
    let Some(size) = preview_size(parent)? else {
        return Ok(false);
    };
    if window.inner_size() != size
        && unsafe {
            SetWindowPos(
                window_hwnd(window)?,
                ptr::null_mut(),
                0,
                0,
                size.width as i32,
                size.height as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        } == 0
    {
        return Err(format!(
            "Cannot resize preview: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(true)
}

pub fn cursor_position() -> Result<(f64, f64), String> {
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return Err(format!(
            "Cannot read screensaver cursor: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok((f64::from(point.x), f64::from(point.y)))
}

pub fn configure(parent: Option<usize>, port: u16) -> Result<(), String> {
    if let Some(parent) = parent
        && unsafe { IsWindow(hwnd(parent)) } == 0
    {
        return Err("Configuration parent HWND is not a live window".into());
    }
    let text: Vec<u16> = format!(
        "Herdr mesh screensaver\n\nReads the local daemon at 127.0.0.1:{port}.\n\
         No settings are saved. Windows launches use port 8790.\n\
         Start the existing herdr-mesh daemon before use.\n\
         Windows controls the wait time and sign-in-on-resume policy."
    )
    .encode_utf16()
    .chain(Some(0))
    .collect();
    let title: Vec<u16> = "Herdr mesh screensaver"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    if unsafe {
        MessageBoxW(
            parent.map_or(ptr::null_mut(), hwnd),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        )
    } == 0
    {
        return Err(format!(
            "Cannot show configuration: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SendMessageW, WS_POPUP,
    };

    #[test]
    fn session_lock_and_suspend_exit_but_unrelated_messages_do_not() {
        assert!(should_exit_for_message(
            WM_POWERBROADCAST,
            PBT_APMSUSPEND as usize
        ));
        assert!(should_exit_for_message(
            WM_WTSSESSION_CHANGE,
            WTS_SESSION_LOCK as usize
        ));
        assert!(!should_exit_for_message(WM_POWERBROADCAST, 0));
        assert!(!should_exit_for_message(0, WTS_SESSION_LOCK as usize));
    }

    #[test]
    fn invalid_preview_and_configuration_parents_fail_explicitly() {
        assert!(preview_size(0).unwrap().is_none());
        assert!(preview_attributes(0).is_err());
        assert!(configure(Some(0), 8790).is_err());
    }

    #[test]
    fn preview_uses_live_parent_client_area_and_detects_destruction() {
        let class: Vec<u16> = "STATIC".encode_utf16().chain(Some(0)).collect();
        // An invisible system-class window exercises HWND validation without a desktop interaction.
        let parent = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                WS_POPUP,
                0,
                0,
                320,
                180,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };

        assert!(!parent.is_null());
        let size = preview_size(parent as usize);
        let attributes = preview_attributes(parent as usize);
        let destroyed = unsafe { DestroyWindow(parent) };
        assert_ne!(destroyed, 0);
        assert_eq!(size.unwrap(), Some(PhysicalSize::new(320, 180)));
        assert!(attributes.is_ok());
        assert!(preview_size(parent as usize).unwrap().is_none());
    }

    #[test]
    fn shutdown_hook_handles_sent_messages_and_releases_its_native_reference() {
        let class: Vec<u16> = "STATIC".encode_utf16().chain(Some(0)).collect();
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                WS_POPUP,
                0,
                0,
                10,
                10,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        assert!(!window.is_null());
        let requested = Rc::new(Cell::new(false));
        let hook = Rc::new(ShutdownHook {
            requested: requested.clone(),
            attached: Cell::new(false),
        });
        let native = Rc::into_raw(hook.clone());
        let installed =
            unsafe { SetWindowSubclass(window, Some(shutdown_proc), SUBCLASS_ID, native as usize) };
        if installed == 0 {
            unsafe {
                drop(Rc::from_raw(native));
                DestroyWindow(window);
            }
            panic!("Cannot install test shutdown hook");
        }
        hook.attached.set(true);
        unsafe {
            SendMessageW(window, WM_POWERBROADCAST, PBT_APMSUSPEND as usize, 0);
        }
        let suspended = requested.replace(false);
        unsafe {
            SendMessageW(window, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as usize, 0);
        }
        let locked = requested.get();
        let destroyed = unsafe { DestroyWindow(window) };
        assert_ne!(destroyed, 0);
        assert!(suspended && locked);
        assert!(!hook.attached.get());
        assert_eq!(Rc::strong_count(&hook), 1);
    }
}
