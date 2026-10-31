//! Hyprland Mod — friendly settings for Hyprland (GTK4 / libadwaita UI).

mod app;
mod apply_bar;
mod backend;
mod backups_page;
mod colors;
mod presets_page;
mod preview;
mod rows;
mod search_page;
mod settings_page;
mod sidebar;
mod transfer_page;
mod window;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
