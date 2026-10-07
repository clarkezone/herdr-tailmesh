mod animation;
mod fleet_panel;
mod launch;
mod mesh_legend;
mod mesh_model;
mod mesh_orb;
mod mesh_stats;
mod mesh_territory;
mod orb_ui;
mod orb_viewport;
mod orbital_sphere;
mod shell;
mod ui;
#[cfg(target_os = "windows")]
mod windows_screensaver;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    shell::run()
}
