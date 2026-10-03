mod animation;
mod fleet_panel;
mod launch;
mod shell;
mod ui;
#[cfg(target_os = "windows")]
mod windows_screensaver;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    shell::run()
}
