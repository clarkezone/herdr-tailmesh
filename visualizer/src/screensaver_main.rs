#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod animation;
mod compact_window;
mod fleet_panel;
mod launch;
mod mesh_legend;
mod mesh_model;
mod mesh_orb;
mod mesh_stats;
mod mesh_territory;
mod orb_focus;
mod orb_hud;
mod orb_objects;
mod orb_panels;
mod orb_ui;
mod orb_viewport;
mod orbital_sphere;
mod shell;
mod ui;
#[cfg(target_os = "windows")]
mod windows_screensaver;

fn main() {
    if let Err(error) = shell::run() {
        log::error!("{error}");
        #[cfg(target_os = "windows")]
        show_error(&error.to_string());
        #[cfg(not(target_os = "windows"))]
        eprintln!("{error}");
        std::process::exit(1);
    }

    #[cfg(target_os = "windows")]
    fn show_error(error: &str) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let text: Vec<u16> = error.encode_utf16().chain(Some(0)).collect();
        let title: Vec<u16> = "Herdr mesh screensaver error"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        if unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            )
        } == 0
        {
            log::error!(
                "Cannot show screensaver error dialog: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}
