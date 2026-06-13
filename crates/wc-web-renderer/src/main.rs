use anyhow::Result;
use clap::Parser;
use gtk::gdk;
use gtk::prelude::*;
use gtk_layer_shell::LayerShell;
use wc_web_renderer::{render_spec_from_args, RenderSpec, RendererArgs};
use webkit2gtk::{SettingsExt, WebViewExt};

fn main() -> Result<()> {
    let args = RendererArgs::parse();
    let spec = render_spec_from_args(&args)?;
    if args.dump_spec {
        println!("project={}", spec.project_dir.display());
        println!("file={}", spec.html_file.display());
        println!("uri={}", spec.file_uri);
        println!("width={}", spec.width);
        println!("height={}", spec.height);
        println!("audio={}", spec.audio);
        return Ok(());
    }
    run_renderer(spec)
}

fn run_renderer(spec: RenderSpec) -> Result<()> {
    gtk::init()?;
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Wallpaper Console Web Renderer");
    window.set_decorated(false);
    window.set_accept_focus(false);
    window.set_default_size(spec.width, spec.height);

    window.init_layer_shell();
    window.set_namespace("wallpaper-console-web-renderer");
    window.set_layer(gtk_layer_shell::Layer::Background);
    window.set_keyboard_mode(gtk_layer_shell::KeyboardMode::None);
    window.set_exclusive_zone(0);
    for edge in [
        gtk_layer_shell::Edge::Left,
        gtk_layer_shell::Edge::Right,
        gtk_layer_shell::Edge::Top,
        gtk_layer_shell::Edge::Bottom,
    ] {
        window.set_anchor(edge, true);
    }
    if let Some(output) = spec.output.as_deref() {
        if let Some(monitor) = find_monitor(output) {
            window.set_monitor(&monitor);
        } else if spec.debug {
            eprintln!("web renderer: output not found: {}", output);
        }
    }

    let webview = webkit2gtk::WebView::new();
    if let Some(settings) = WebViewExt::settings(&webview) {
        settings.set_enable_javascript(true);
        settings.set_media_playback_requires_user_gesture(false);
        settings.set_enable_developer_extras(spec.debug);
    }
    webview.set_is_muted(!spec.audio);
    if spec.debug {
        webview.connect_load_failed(|_, _, uri, err| {
            eprintln!("web renderer load failed: {}: {}", uri, err);
            false
        });
        webview.connect_load_changed(|_, event| {
            if event == webkit2gtk::LoadEvent::Finished {
                eprintln!("web renderer load finished");
            }
        });
    }

    webview.load_uri(&spec.file_uri);
    window.add(&webview);
    window.connect_destroy(|_| gtk::main_quit());
    window.show_all();
    gtk::main();
    Ok(())
}

fn find_monitor(name: &str) -> Option<gdk::Monitor> {
    let display = gdk::Display::default()?;
    for idx in 0..display.n_monitors() {
        let monitor = display.monitor(idx)?;
        let mut candidates: Vec<String> = Vec::new();
        if let Some(model) = monitor.model() {
            candidates.push(model.to_string());
        }
        if let Some(manufacturer) = monitor.manufacturer() {
            candidates.push(manufacturer.to_string());
        }
        candidates.push(format!("{:?}", monitor));
        if candidates.iter().any(|c| c.contains(name)) {
            return Some(monitor);
        }
    }
    None
}
