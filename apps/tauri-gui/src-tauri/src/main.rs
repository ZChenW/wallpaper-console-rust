// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if let Some(exit_code) = wc_app::scan_worker::try_run_worker_mode(&args) {
        std::process::exit(exit_code);
    }
    app_lib::run();
}
