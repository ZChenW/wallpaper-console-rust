use std::collections::HashSet;
use std::io::Write;
use std::process::{Command, Stdio};

use wc_storage::StorageApi;

use crate::library::library_paths;
use crate::Commands;

fn parse_display_target(raw: &str) -> anyhow::Result<wc_app::DisplayTarget> {
    let value = raw.trim();
    if value.is_empty() {
        anyhow::bail!("display target must not be blank");
    }
    if value.eq_ignore_ascii_case("all")
        || value.eq_ignore_ascii_case("all displays")
        || value == wc_storage::sqlite::ALL_DISPLAYS_TARGET_KEY
    {
        return Ok(wc_app::DisplayTarget::AllDisplays);
    }
    Ok(wc_app::DisplayTarget::Output(value.to_string()))
}

fn discover_connected_outputs() -> anyhow::Result<Vec<String>> {
    wc_app::discover_connected_outputs().map_err(|error| {
        let detail = error
            .detail
            .map(|detail| format!(" ({detail})"))
            .unwrap_or_default();
        anyhow::anyhow!("{}{}", error.message, detail)
    })
}

fn apply_to_display_with<F>(
    path: &str,
    raw_target: &str,
    known_outputs: &[String],
    apply_targeted: F,
) -> anyhow::Result<wc_app::ApplyTarget>
where
    F: FnOnce(
        &str,
        wc_app::DisplayTarget,
        &[String],
    ) -> Result<wc_app::ApplyTarget, wc_app::AppError>,
{
    let target = parse_display_target(raw_target)?;
    apply_targeted(path, target, known_outputs).map_err(|e| anyhow::anyhow!(e.message))
}

fn restore_displays_with<F>(known_outputs: &[String], restore: F) -> anyhow::Result<()>
where
    F: FnOnce(&[String]) -> Result<(), wc_app::AppError>,
{
    restore(known_outputs).map_err(|e| anyhow::anyhow!(e.message))
}

fn resolve_known_outputs_with<F>(
    explicit_outputs: &[String],
    discover: F,
) -> anyhow::Result<Vec<String>>
where
    F: FnOnce() -> anyhow::Result<Vec<String>>,
{
    let discovered_outputs = discover()?;
    validate_known_outputs(&discovered_outputs)?;

    if !explicit_outputs.is_empty() {
        validate_known_outputs(explicit_outputs)?;
        let explicit: HashSet<_> = explicit_outputs.iter().map(String::as_str).collect();
        let discovered: HashSet<_> = discovered_outputs.iter().map(String::as_str).collect();
        if explicit != discovered {
            anyhow::bail!(
                "explicit display outputs must exactly match discovered connected outputs"
            );
        }
    }

    Ok(discovered_outputs)
}

fn validate_known_outputs(outputs: &[String]) -> anyhow::Result<()> {
    let mut seen = HashSet::new();
    for output in outputs {
        if output.trim().is_empty() {
            anyhow::bail!("display output must not be blank");
        }
        if !seen.insert(output.as_str()) {
            anyhow::bail!("duplicate display output: {output}");
        }
    }
    Ok(())
}

pub(crate) fn apply(
    s: &StorageApi,
    file: String,
    target: Option<String>,
    explicit_outputs: Vec<String>,
) -> anyhow::Result<()> {
    let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
        path: s.cd.path.clone(),
    });
    let target = match target {
        None => {
            if !explicit_outputs.is_empty() {
                anyhow::bail!("--output requires an explicit --target");
            }
            service
                .apply(&file)
                .map_err(|e| anyhow::anyhow!(e.message))?
        }
        Some(raw_target) => {
            let known_outputs =
                resolve_known_outputs_with(&explicit_outputs, discover_connected_outputs)?;
            apply_to_display_with(
                &file,
                &raw_target,
                &known_outputs,
                |path, target, outputs| service.apply_to_display(path, target, outputs),
            )?
        }
    };
    println!("Applied: {}", target.resolved_path);
    Ok(())
}

pub(crate) fn inspect(s: &StorageApi, path: String) -> anyhow::Result<()> {
    let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
        path: s.cd.path.clone(),
    });
    let inspected = service
        .inspect_path(&path)
        .map_err(|e| anyhow::anyhow!(serde_json::to_string_pretty(&e).unwrap_or(e.message)))?;
    println!("{}", serde_json::to_string_pretty(&inspected)?);
    Ok(())
}

pub(crate) fn stop(s: &StorageApi) -> anyhow::Result<()> {
    stop_wallpapers_with(s, wc_backend::stop_all_backends)?;
    println!("All wallpaper backends stopped.");
    Ok(())
}

pub(crate) fn status(s: &StorageApi) -> anyhow::Result<()> {
    let cur = s.current_read()?.unwrap_or_else(|| "(none)".into());
    let be = s.last_backend_read()?.unwrap_or_else(|| "(none)".into());
    let src_count = s.sources_list()?.len();
    println!("config directory:    {}", s.cd.path.display());
    println!("current wallpaper:   {}", cur);
    println!("last backend:        {}", be);
    println!("configured sources:  {}", src_count);
    Ok(())
}

pub(crate) fn restore(s: &StorageApi) -> anyhow::Result<()> {
    wc_backend::restore_clean(s)?;
    println!("Wallpaper restored.");
    Ok(())
}

pub(crate) fn displays() -> anyhow::Result<()> {
    let outputs = discover_connected_outputs()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&crate::output::json_from_display_names(&outputs))?
    );
    Ok(())
}

pub(crate) fn display_state(s: &StorageApi) -> anyhow::Result<()> {
    let rows = s.display_state_list()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&crate::output::json_from_display_state_rows(&rows))?
    );
    Ok(())
}

pub(crate) fn restore_displays(
    s: &StorageApi,
    explicit_outputs: Vec<String>,
) -> anyhow::Result<()> {
    let known_outputs = resolve_known_outputs_with(&explicit_outputs, discover_connected_outputs)?;
    let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
        path: s.cd.path.clone(),
    });
    restore_displays_with(&known_outputs, |outputs| service.restore_displays(outputs))?;
    println!("Display wallpapers restored.");
    Ok(())
}

pub(crate) fn run(cmd: Commands, s: &StorageApi) -> anyhow::Result<()> {
    match cmd {
        // ── Browse (fzf interactive, apply on selection) ─────────────
        Commands::Browse
        | Commands::BrowseAll
        | Commands::BrowseImages
        | Commands::BrowseGifs
        | Commands::BrowseVideos => {
            let (filter, label) = match &cmd {
                Commands::BrowseImages => (Some(wc_core::types::FileType::Image), "browse-images"),
                Commands::BrowseGifs => (Some(wc_core::types::FileType::Gif), "browse-gifs"),
                Commands::BrowseVideos => (Some(wc_core::types::FileType::Video), "browse-videos"),
                _ => (None, "browse"),
            };
            let candidates = scan_paths(s, filter)?;
            if candidates.is_empty() {
                anyhow::bail!("no wallpapers found");
            }
            let selection = fzf_select(&candidates, &format!("{}> ", label))?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        // ── Random ───────────────────────────────────────────────────
        Commands::Random
        | Commands::RandomAll
        | Commands::RandomImage
        | Commands::RandomGif
        | Commands::RandomVideo => {
            let filter = match &cmd {
                Commands::RandomImage => Some(wc_core::types::FileType::Image),
                Commands::RandomGif => Some(wc_core::types::FileType::Gif),
                Commands::RandomVideo => Some(wc_core::types::FileType::Video),
                _ => None,
            };
            let paths = library_paths(s, filter)?;
            if paths.is_empty() {
                anyhow::bail!("no matching wallpapers found");
            }
            let idx = rand::random::<usize>() % paths.len();
            let chosen = &paths[idx];
            apply_selected(s, chosen)?;
        }

        // ── Sources ──────────────────────────────────────────────────
        Commands::Add { dir } => {
            let canonical = std::fs::canonicalize(&dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or(dir);
            s.sources_add(&canonical)?;
            println!("Added source: {}", canonical);
        }

        Commands::Sources => {
            let srcs = s.sources_list()?;
            if srcs.is_empty() {
                println!("(no source directories configured)");
            } else {
                for src in &srcs {
                    println!("{}", src);
                }
            }
        }

        Commands::Remove => {
            let paths = s.sources_list()?;
            if paths.is_empty() {
                anyhow::bail!("no sources configured");
            }
            let selection = fzf_select(&paths, "remove source> ")?;
            if let Some(path) = selection {
                s.sources_remove(&path)?;
                println!("Removed source: {}", path);
            }
        }

        Commands::RemoveSource { dir } => {
            // Try exact match first (works even when dir no longer exists).
            let removed = s.sources_remove(&dir)?;
            if removed {
                println!("Removed source: {}", dir);
                return Ok(());
            }
            // Canonicalise and scan stored sources for a match.
            let canonical = std::fs::canonicalize(&dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| dir.clone());
            let sources = s.sources_list()?;
            for stored in &sources {
                let stored_canon = std::fs::canonicalize(stored)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| stored.clone());
                if stored_canon == canonical {
                    s.sources_remove(stored)?;
                    println!("Removed source: {}", stored);
                    return Ok(());
                }
            }
            anyhow::bail!("source not found: {}", dir);
        }

        Commands::SteamWorkshop => {
            let home = std::env::var("HOME").unwrap_or_default();
            for root in wc_scan::discover_steam_workshop_roots(std::path::Path::new(&home)) {
                let canonical = root.to_string_lossy().to_string();
                if s.sources_add(&canonical)? {
                    println!("Added: {}", canonical);
                }
            }
            println!("Steam Workshop scan complete.");
        }

        Commands::ValidateSources => {
            for src in s.sources_list()? {
                let exists = std::path::Path::new(&src).is_dir();
                println!("{}  {}", if exists { "✓" } else { "✕" }, src);
            }
        }

        Commands::RemoveMissing => {
            let sources = s.sources_list()?;
            let mut removed = 0;
            for src in &sources {
                if !std::path::Path::new(src).is_dir() {
                    s.sources_remove(src)?;
                    println!("Removed missing source: {}", src);
                    removed += 1;
                }
            }
            println!("Removed {} missing source(s).", removed);
        }

        Commands::DedupeSources => {
            wc_storage::flat::dedupe_file(&s.cd.sources_path())?;
            println!("Sources deduplicated.");
        }

        // ── Favorites ────────────────────────────────────────────────
        Commands::FavoriteAdd { file } => {
            let added = s.favorites_add(&file)?;
            if added {
                println!("Added to favorites: {}", file);
            } else {
                println!("Already in favorites");
            }
        }

        Commands::FavoriteAddCurrent => {
            if let Some(cur) = s.current_read()? {
                s.favorites_add(&cur)?;
                println!("Added to favorites: {}", cur);
            } else {
                anyhow::bail!("no current wallpaper (apply one first)");
            }
        }

        Commands::Favorites => {
            let favs = s.favorites_list()?;
            if favs.is_empty() {
                println!("(no favorites)");
                return Ok(());
            }
            let selection = fzf_select(&favs, "favorites> ")?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        Commands::FavoriteRandom => {
            let favs = s.favorites_list()?;
            if favs.is_empty() {
                anyhow::bail!("no favorites configured");
            }
            let idx = rand::random::<usize>() % favs.len();
            let chosen = &favs[idx];
            apply_selected(s, chosen)?;
        }

        Commands::FavoriteRemove { file } => {
            if let Some(path) = file {
                s.favorites_remove(&path)?;
                println!("Removed favorite: {}", path);
            } else {
                let favs = s.favorites_list()?;
                if favs.is_empty() {
                    anyhow::bail!("no favorites configured");
                }
                let selection = fzf_select(&favs, "remove favorite> ")?;
                if let Some(path) = selection {
                    s.favorites_remove(&path)?;
                    println!("Removed favorite: {}", path);
                }
            }
        }

        // ── Search / Sort ────────────────────────────────────────────
        Commands::Search { query } => {
            let q = resolve_query(&query, "Search query")?;
            let candidates = scan_paths_matching_filename(s, &q)?;
            if candidates.is_empty() {
                anyhow::bail!("no wallpapers matching: {}", q);
            }
            let selection = fzf_select(&candidates, &format!("search:{}> ", q))?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        Commands::SearchSource { query } => {
            let q = resolve_query(&query, "Source query")?;
            let candidates = scan_paths_matching_source(s, &q)?;
            if candidates.is_empty() {
                anyhow::bail!("no sources matching: {}", q);
            }
            let selection = fzf_select(&candidates, &format!("search-source:{}> ", q))?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        Commands::SearchType { query } => {
            let q = resolve_query(&query, "Type (image/gif/video/we_scene/we_web)")?;
            let filter = match q.to_lowercase().as_str() {
                "image" => Some(wc_core::types::FileType::Image),
                "gif" => Some(wc_core::types::FileType::Gif),
                "video" => Some(wc_core::types::FileType::Video),
                "we_scene" | "scene" => Some(wc_core::types::FileType::WeScene),
                "we_web" | "web" => Some(wc_core::types::FileType::WeWeb),
                other => {
                    anyhow::bail!(
                        "unknown type '{}' — use image, gif, video, we_scene, or we_web",
                        other
                    )
                }
            };
            // Live scan (matches Bash: scan_wallpapers_by_type), not library.tsv
            let candidates = scan_paths(s, filter)?;
            if candidates.is_empty() {
                anyhow::bail!("no wallpapers of type: {}", q);
            }
            let selection = fzf_select(&candidates, &format!("search-type:{}> ", q))?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        Commands::SortMtime | Commands::SortSize | Commands::SortName => {
            let candidates = scan_paths(s, None)?;
            if candidates.is_empty() {
                anyhow::bail!("no wallpapers found");
            }
            let sorted = sort_paths(&candidates, &cmd);
            let label = match &cmd {
                Commands::SortMtime => "sort:mtime",
                Commands::SortSize => "sort:size",
                Commands::SortName => "sort:name",
                _ => "sort",
            };
            let selection = fzf_select(&sorted, &format!("{}> ", label))?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        // ── Config ───────────────────────────────────────────────────
        Commands::ConfigGet { key, default } => {
            let val = s.config_get(&key, &default.unwrap_or_default());
            println!("{}", val);
        }

        Commands::ConfigSet { key, value } => {
            let val = value.join(" ");
            s.config_set(&key, &val)?;
            println!("{} = {}", key, val);
        }

        Commands::Tui => {
            println!("TUI not yet implemented in Rust — use the Bash wallpaper-console for TUI.");
        }

        Commands::Preview { file } => {
            wc_preview::render_preview(&s.cd, &file);
        }

        Commands::Thumbnail { file } => {
            let cache_dir = s.cd.gui_thumbnail_cache_dir();
            let result = wc_preview::thumbnail_for(&cache_dir, &file);
            if let Some(thumb) = result.thumbnail {
                println!("{}", thumb);
            } else if let Some(err) = result.error {
                eprintln!("{}", err);
                std::process::exit(1);
            } else {
                eprintln!("thumbnail generation failed");
                std::process::exit(1);
            }
        }
        Commands::ThumbnailBatch { files } => {
            let cache_dir = s.cd.gui_thumbnail_cache_dir();
            let results: Vec<serde_json::Value> = files
                .into_iter()
                .map(|path| {
                    let result = wc_preview::thumbnail_for(&cache_dir, &path);
                    let mut obj = serde_json::json!({
                        "path": path,
                        "cacheHit": result.cache_hit,
                    });
                    if let Some(thumb) = result.thumbnail {
                        obj["thumbnail"] = serde_json::json!(thumb);
                    }
                    obj
                })
                .collect();
            println!("{}", serde_json::to_string(&results)?);
        }

        _ => unreachable!("wallpaper::run called with non-wallpaper command"),
    }
    Ok(())
}

pub(crate) fn stop_wallpapers_with<F>(
    s: &StorageApi,
    stop_backends: F,
) -> Result<(), wc_core::error::WcError>
where
    F: FnOnce(Option<&StorageApi>) -> Result<(), wc_core::error::WcError>,
{
    stop_backends(Some(s))?;
    s.runtime_state_clear()
}

pub(crate) fn apply_selected(s: &StorageApi, path: &str) -> anyhow::Result<()> {
    let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
        path: s.cd.path.clone(),
    });
    let target = service
        .apply(path)
        .map_err(|e| anyhow::anyhow!(e.message))?;
    println!("Applied: {}", target.resolved_path);
    Ok(())
}

/// Live-scan all sources for wallpaper paths (bypasses library.tsv cache).
fn scan_paths(
    s: &StorageApi,
    filter: Option<wc_core::types::FileType>,
) -> anyhow::Result<Vec<String>> {
    let sources = s.sources_list()?;
    let all = wc_scan::scan_wallpapers(&sources);
    if let Some(ft) = filter {
        Ok(all
            .into_iter()
            .filter(|p| {
                wc_scan::make_entry(p)
                    .map(|entry| entry.file_type == ft)
                    .unwrap_or(false)
            })
            .collect())
    } else {
        Ok(all)
    }
}

/// Live-scan and filter by filename (case-insensitive substring match).
fn scan_paths_matching_filename(s: &StorageApi, query: &str) -> anyhow::Result<Vec<String>> {
    let sources = s.sources_list()?;
    let all = wc_scan::scan_wallpapers(&sources);
    let q = query.to_lowercase();
    Ok(all
        .into_iter()
        .filter(|p| {
            std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_lowercase().contains(&q))
                .unwrap_or(false)
        })
        .collect())
}

/// Live-scan sources whose path contains the query, return all files.
fn scan_paths_matching_source(s: &StorageApi, query: &str) -> anyhow::Result<Vec<String>> {
    let sources = s.sources_list()?;
    let q = query.to_lowercase();
    let matching: Vec<String> = sources
        .into_iter()
        .filter(|src| src.to_lowercase().contains(&q))
        .collect();
    if matching.is_empty() {
        return Ok(Vec::new());
    }
    Ok(wc_scan::scan_wallpapers(&matching))
}

fn sort_paths(candidates: &[String], cmd: &Commands) -> Vec<String> {
    // Build (key, path) pairs
    let mut pairs: Vec<(String, String)> = candidates
        .iter()
        .map(|p| {
            let key = match cmd {
                Commands::SortMtime => {
                    let m = std::fs::metadata(p)
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    format!("{:020}", m)
                }
                Commands::SortSize => {
                    let s = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                    format!("{:020}", s)
                }
                Commands::SortName => std::path::Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase(),
                _ => String::new(),
            };
            (key, p.clone())
        })
        .collect();

    match cmd {
        Commands::SortMtime | Commands::SortSize => {
            pairs.sort_by(|a, b| b.0.cmp(&a.0)); // descending
        }
        Commands::SortName => {
            pairs.sort_by(|a, b| a.0.cmp(&b.0)); // ascending
        }
        _ => {}
    }
    pairs.into_iter().map(|(_, p)| p).collect()
}

/// Get the query string, prompting interactively if empty.
fn resolve_query(args: &[String], prompt: &str) -> anyhow::Result<String> {
    if !args.is_empty() {
        return Ok(args.join(" "));
    }
    use std::io::{BufRead, Write};
    let mut stderr = std::io::stderr();
    write!(stderr, "{}: ", prompt)?;
    stderr.flush()?;
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    let trimmed = line.trim().to_string();
    if trimmed.is_empty() {
        anyhow::bail!("cancelled");
    }
    Ok(trimmed)
}

pub(crate) fn fzf_select(items: &[String], prompt: &str) -> anyhow::Result<Option<String>> {
    let self_path = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("wallpaper-console-rust"));
    let preview_cmd = format!("{} __preview__ {{}}", self_path.to_string_lossy());

    let mut child = Command::new("fzf")
        .arg("--prompt")
        .arg(prompt)
        .arg("--preview")
        .arg(&preview_cmd)
        .arg("--preview-window=right:60%:wrap")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        for item in items {
            writeln!(stdin, "{}", item)?;
        }
    }
    let output = child.wait_with_output()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.is_empty() {
            Ok(None)
        } else {
            Ok(Some(s))
        }
    } else {
        // fzf exits 130 on Ctrl-C / Esc
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_storage::StorageApi;

    fn storage_with_mode(mode: &str) -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", mode).unwrap();
        let storage = StorageApi::new(cd);
        (tmp, storage)
    }

    fn no_op_stop(_s: Option<&StorageApi>) -> Result<(), wc_core::error::WcError> {
        Ok(())
    }

    #[test]
    fn explicit_display_target_parses_all_or_a_named_output() {
        assert_eq!(
            parse_display_target("all").unwrap(),
            wc_app::DisplayTarget::AllDisplays
        );
        assert_eq!(
            parse_display_target("__all_displays__").unwrap(),
            wc_app::DisplayTarget::AllDisplays
        );
        assert_eq!(
            parse_display_target(" eDP-1 ").unwrap(),
            wc_app::DisplayTarget::Output("eDP-1".into())
        );
        assert!(parse_display_target("  ").is_err());
    }

    #[test]
    fn targeted_apply_delegates_target_and_outputs_without_routing_a_backend() {
        let known_outputs = vec!["eDP-1".to_string(), "HDMI-A-1".to_string()];
        let result = apply_to_display_with(
            "/walls/video.mp4",
            "eDP-1",
            &known_outputs,
            |path, target, outputs| {
                assert_eq!(path, "/walls/video.mp4");
                assert_eq!(target, wc_app::DisplayTarget::Output("eDP-1".into()));
                assert_eq!(outputs, ["eDP-1", "HDMI-A-1"]);
                Ok(wc_app::ApplyTarget {
                    input_path: path.to_string(),
                    resolved_path: path.to_string(),
                    file_type: wc_core::types::FileType::Video,
                    backend: wc_core::types::Backend::Mpvpaper,
                })
            },
        )
        .unwrap();

        assert_eq!(result.backend, wc_core::types::Backend::Mpvpaper);
    }

    #[test]
    fn targeted_restore_delegates_the_connected_output_set_to_wc_app() {
        let known_outputs = vec!["eDP-1".to_string(), "HDMI-A-1".to_string()];
        restore_displays_with(&known_outputs, |outputs| {
            assert_eq!(outputs, ["eDP-1", "HDMI-A-1"]);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn explicit_outputs_must_match_discovery_order_independently() {
        let explicit = vec!["eDP-1".to_string(), "HDMI-A-1".to_string()];
        let resolved =
            resolve_known_outputs_with(&explicit, || Ok(vec!["HDMI-A-1".into(), "eDP-1".into()]))
                .unwrap();
        assert_eq!(resolved, ["HDMI-A-1", "eDP-1"]);

        let discovered = resolve_known_outputs_with(&[], || Ok(vec!["DP-1".into()])).unwrap();
        assert_eq!(discovered, ["DP-1"]);
    }

    #[test]
    fn explicit_outputs_reject_incomplete_or_extra_sets() {
        let incomplete = resolve_known_outputs_with(&["eDP-1".into()], || {
            Ok(vec!["eDP-1".into(), "HDMI-A-1".into()])
        })
        .unwrap_err();
        assert!(incomplete.to_string().contains("match"), "{incomplete}");

        let extra = resolve_known_outputs_with(&["eDP-1".into(), "HDMI-A-1".into()], || {
            Ok(vec!["eDP-1".into()])
        })
        .unwrap_err();
        assert!(extra.to_string().contains("match"), "{extra}");
    }

    #[test]
    fn explicit_outputs_reject_discovery_failure_and_invalid_values() {
        let discovery_error = resolve_known_outputs_with(&["eDP-1".into()], || {
            anyhow::bail!("real display discovery failed")
        })
        .unwrap_err();
        assert!(
            discovery_error.to_string().contains("discovery failed"),
            "{discovery_error}"
        );

        assert!(resolve_known_outputs_with(&["  ".into()], || Ok(vec!["eDP-1".into()])).is_err());
        assert!(
            resolve_known_outputs_with(&["eDP-1".into(), "eDP-1".into()], || {
                Ok(vec!["eDP-1".into()])
            })
            .is_err()
        );
    }

    #[test]
    fn stop_wallpapers_clears_file_runtime_state_but_keeps_history() {
        let (_tmp, storage) = storage_with_mode("file");
        storage.current_write("/walls/current.jpg").unwrap();
        storage.last_backend_write("awww").unwrap();
        storage.history_add("/walls/current.jpg", "awww").unwrap();

        stop_wallpapers_with(&storage, no_op_stop).unwrap();

        assert_eq!(storage.current_read().unwrap(), None);
        assert_eq!(storage.last_backend_read().unwrap(), None);
        assert_eq!(
            storage.history_list().unwrap(),
            vec!["/walls/current.jpg".to_string()]
        );
    }

    #[test]
    fn stop_wallpapers_clears_sqlite_runtime_state_but_keeps_history() {
        let (_tmp, storage) = storage_with_mode("sqlite");
        storage.current_write("/walls/current.jpg").unwrap();
        storage.last_backend_write("awww").unwrap();
        storage.history_add("/walls/current.jpg", "awww").unwrap();

        stop_wallpapers_with(&storage, no_op_stop).unwrap();

        assert_eq!(storage.current_read().unwrap(), None);
        assert_eq!(storage.last_backend_read().unwrap(), None);
        assert_eq!(
            storage.history_list().unwrap(),
            vec!["/walls/current.jpg".to_string()]
        );
    }
}
