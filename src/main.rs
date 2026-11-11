#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]

mod data;
mod detect;
mod layout;
mod live_client;
mod picks;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod config;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod update;

fn main() {
    #[cfg(windows)]
    app::run().expect("mayhem-overlay");
}
