//! Photon: screenshots, markup and screen recording for Apple silicon Macs.


mod annotate;
mod app;
mod capture;
mod commands;
mod export;
mod geometry;
mod hotkey;
mod hud;
mod i18n;
mod i18n_text;
mod keys;
mod output;
mod overlay;
mod permissions;
mod pin;
mod record;
mod selftest;
mod session;
mod settings;
mod settings_window;
mod ui;
mod update;
mod update_ui;
mod util;

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
compile_error!("Photon is built for Apple silicon Macs only (aarch64-apple-darwin).");

fn main() {
    // `Photon --login-item on|off|status` manages Launch at Login without the UI.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--login-item") {
        let result = match args.get(1).map(String::as_str) {
            Some("on") => app::set_login_item(true),
            Some("off") => app::set_login_item(false),
            _ => Ok(()),
        };
        match result {
            Ok(()) => println!("Launch at Login: {}", if app::login_item_enabled() { "on" } else { "off" }),
            Err(e) => {
                eprintln!("Couldn't change Launch at Login: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    app::run();
}
