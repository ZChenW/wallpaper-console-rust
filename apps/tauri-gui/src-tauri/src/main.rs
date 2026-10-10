// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if let Some(exit_code) = wc_backend::image_media::try_run_worker_mode(&args) {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) = wc_app::scan_worker::try_run_worker_mode(&args) {
        std::process::exit(exit_code);
    }
    prefer_software_video_sink();
    app_lib::run();
}

/// WebKitGTK's GL video sink draws every frame of a `<video>` as one flat green on the NVIDIA
/// proprietary driver (verified on an RTX 5060 under niri: mean colour 0,75,0, no variation, while
/// the clip's time advances normally). Its software sink draws the same clip correctly. The setting
/// touches nothing but `<video>` elements, which only the Knot's preview clips use, and it must be
/// in the environment before the web process starts. A value the user has set is left alone.
fn prefer_software_video_sink() {
    const KEY: &str = "WEBKIT_GST_DISABLE_GL_SINK";
    if std::env::var_os(KEY).is_none() {
        std::env::set_var(KEY, "1");
    }
}
