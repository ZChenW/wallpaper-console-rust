//! Source traversal, cancellation and Wallpaper Engine source normalization.
use crate::project::indexed_we_project_path;
use std::{collections::HashSet, fs, path::Path};
use walkdir::WalkDir;
use wc_core::formats;
pub(super) const CANCEL_CHECK_INTERVAL: usize = 500;
const WE_MARKER: &str = "/steamapps/workshop/content/431960";
/// Flatpak Steam installs workshop content under a different prefix.
const FLATPAK_WE_MARKER: &str =
    "/.var/app/com.valvesoftware.Steam/data/Steam/steamapps/workshop/content/431960";

/// Deduplicate sources by canonical path before scanning.
pub fn dedupe_sources(sources: &[String]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut result: Vec<String> = Vec::new();
    for src in sources {
        let p = Path::new(src);
        if !p.is_dir() {
            continue;
        }
        let canon = std::fs::canonicalize(p)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| src.clone());
        if seen.insert(canon.clone()) {
            result.push(canon);
        }
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanEvent {
    SourceStarted { source: String },
    CandidateFound { path: String, count: usize },
    WalkProgress { entries_visited: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanControl {
    Continue,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanVisitControl {
    Continue,
    Cancel,
}

enum CandidateSink<'a> {
    Collect(&'a mut Vec<String>),
    Stream,
}

impl CandidateSink<'_> {
    fn push_if_collecting(&mut self, path: String) {
        if let CandidateSink::Collect(files) = self {
            files.push(path);
        }
    }
}

/// Scan all sources for wallpaper files with a callback for progress/cancellation.
pub fn scan_wallpapers_with_callback<F>(sources: &[String], mut on_event: F) -> Vec<String>
where
    F: FnMut(ScanEvent) -> ScanControl,
{
    let mut files: Vec<String> = Vec::new();
    let mut sink = CandidateSink::Collect(&mut files);
    let mut noop = |_| ScanVisitControl::Continue;
    scan_wallpapers_with_visitor(sources, &mut sink, &mut 0usize, &mut on_event, &mut noop);
    files
}

/// Legacy compatibility wrapper — scans without cancellation support.
pub fn scan_wallpapers(sources: &[String]) -> Vec<String> {
    scan_wallpapers_with_callback(sources, |_| ScanControl::Continue)
}

/// Stream wallpaper paths through a visitor callback. Paths are yielded
/// as they are discovered — no full Vec is accumulated in this call.
/// Returns true if the visitor cancelled.
pub fn visit_wallpapers_with_callback<F, V>(
    sources: &[String],
    mut on_event: F,
    mut on_candidate: V,
) -> bool
where
    F: FnMut(ScanEvent) -> ScanControl,
    V: FnMut(String) -> ScanVisitControl,
{
    let mut cancelled = false;
    let mut sink = CandidateSink::Stream;
    let mut wrapper = |path| {
        if matches!(on_candidate(path), ScanVisitControl::Cancel) {
            cancelled = true;
            ScanVisitControl::Cancel
        } else {
            ScanVisitControl::Continue
        }
    };
    scan_wallpapers_with_visitor(sources, &mut sink, &mut 0usize, &mut on_event, &mut wrapper);
    cancelled
}

fn scan_wallpapers_with_visitor<F, V>(
    sources: &[String],
    sink: &mut CandidateSink<'_>,
    count: &mut usize,
    on_event: &mut F,
    on_candidate: &mut V,
) where
    F: FnMut(ScanEvent) -> ScanControl,
    V: FnMut(String) -> ScanVisitControl,
{
    let deduped = dedupe_sources(sources);
    let mut seen: HashSet<String> = HashSet::new();

    for source in &deduped {
        if matches!(
            on_event(ScanEvent::SourceStarted {
                source: source.clone()
            }),
            ScanControl::Cancel
        ) {
            break;
        }
        let src_path = Path::new(source);
        if !src_path.is_dir() {
            continue;
        }
        let cancelled = match we_source_kind(source) {
            WeKind::WorkshopRoot => scan_we_workshop_root_with_callback(
                src_path,
                &mut seen,
                sink,
                count,
                on_event,
                on_candidate,
            ),
            WeKind::ProjectDir => scan_we_project_dir_with_callback(
                src_path,
                &mut seen,
                sink,
                count,
                on_event,
                on_candidate,
            ),
            WeKind::Normal => scan_dir_recursive_with_callback(
                src_path,
                &mut seen,
                sink,
                count,
                on_event,
                on_candidate,
            ),
        };
        if cancelled {
            break;
        }
    }
}

/// Classify a WE source path.
#[derive(Debug, PartialEq)]
pub(super) enum WeKind {
    /// .../431960 — iterate subdirectories as projects.
    WorkshopRoot,
    /// .../431960/<project_id> — read this project's project.json.
    ProjectDir,
    /// Not a WE path at all.
    Normal,
}

pub(super) fn find_we_marker(path: &str) -> Option<(usize, &'static str)> {
    for marker in [WE_MARKER, FLATPAK_WE_MARKER] {
        for (position, _) in path.match_indices(marker) {
            let end = position + marker.len();
            if end == path.len() || path.as_bytes().get(end) == Some(&b'/') {
                return Some((position, marker));
            }
        }
    }
    None
}

pub(super) fn we_source_kind(path: &str) -> WeKind {
    let Some((pos, marker)) = find_we_marker(path) else {
        return WeKind::Normal;
    };
    let after = path[pos + marker.len()..].trim_start_matches('/');
    if !after.is_empty() {
        let first_seg = after.split('/').next().unwrap_or("");
        if first_seg.chars().all(|c| c.is_ascii_digit()) {
            return WeKind::ProjectDir;
        }
    }
    WeKind::WorkshopRoot
}

/// Scan a Wallpaper Engine workshop root with cancellation support.
fn scan_we_workshop_root_with_callback<F, V>(
    root: &Path,
    seen: &mut HashSet<String>,
    sink: &mut CandidateSink<'_>,
    count: &mut usize,
    on_event: &mut F,
    on_candidate: &mut V,
) -> bool
where
    F: FnMut(ScanEvent) -> ScanControl,
    V: FnMut(String) -> ScanVisitControl,
{
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return false,
    };

    let mut visited = 0usize;

    for entry in entries.filter_map(|e| e.ok()) {
        visited += 1;
        if visited.is_multiple_of(CANCEL_CHECK_INTERVAL)
            && matches!(
                on_event(ScanEvent::WalkProgress {
                    entries_visited: visited
                }),
                ScanControl::Cancel
            )
        {
            return true;
        }

        let ftype = entry.file_type().ok();
        if !ftype.map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let project_dir = entry.path();

        let has_proj = project_dir.join("project.json").exists();
        let we_file = indexed_we_project_path(&project_dir);

        if let Some(ref wp) = we_file {
            let p = Path::new(wp);
            let c = canonicalize_str(p);
            if seen.insert(c.clone()) {
                sink.push_if_collecting(c.clone());
                *count += 1;
                if matches!(
                    on_event(ScanEvent::CandidateFound {
                        path: c.clone(),
                        count: *count
                    }),
                    ScanControl::Cancel
                ) {
                    return true;
                }
                if matches!(on_candidate(c), ScanVisitControl::Cancel) {
                    return true;
                }
            }
            continue;
        }

        if has_proj {
            continue;
        }

        let cancelled = scan_dir_recursive_with_callback(
            &project_dir,
            seen,
            sink,
            count,
            on_event,
            on_candidate,
        );
        if cancelled {
            return true;
        }
    }
    false
}

/// Scan a single WE project directory with cancellation support.
fn scan_we_project_dir_with_callback<F, V>(
    project_dir: &Path,
    seen: &mut HashSet<String>,
    sink: &mut CandidateSink<'_>,
    count: &mut usize,
    on_event: &mut F,
    on_candidate: &mut V,
) -> bool
where
    F: FnMut(ScanEvent) -> ScanControl,
    V: FnMut(String) -> ScanVisitControl,
{
    if let Some(wp) = indexed_we_project_path(project_dir) {
        let p = Path::new(&wp);
        let c = canonicalize_str(p);
        if seen.insert(c.clone()) {
            sink.push_if_collecting(c.clone());
            *count += 1;
            if matches!(
                on_event(ScanEvent::CandidateFound {
                    path: c.clone(),
                    count: *count
                }),
                ScanControl::Cancel
            ) {
                return true;
            }
            if matches!(on_candidate(c), ScanVisitControl::Cancel) {
                return true;
            }
        }
    }
    false
}

/// Recursively scan a directory for supported wallpaper files with cancellation support.
fn scan_dir_recursive_with_callback<F, V>(
    dir: &Path,
    seen: &mut HashSet<String>,
    sink: &mut CandidateSink<'_>,
    count: &mut usize,
    on_event: &mut F,
    on_candidate: &mut V,
) -> bool
where
    F: FnMut(ScanEvent) -> ScanControl,
    V: FnMut(String) -> ScanVisitControl,
{
    let mut visited = 0usize;

    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        visited += 1;
        if visited.is_multiple_of(CANCEL_CHECK_INTERVAL)
            && matches!(
                on_event(ScanEvent::WalkProgress {
                    entries_visited: visited
                }),
                ScanControl::Cancel
            )
        {
            return true;
        }

        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if formats::is_preview_filename(name) {
            continue;
        }
        if let Some(ext) = formats::get_extension(&path.to_string_lossy()) {
            if formats::is_supported_extension(&ext) {
                let canonical = canonicalize_str(path);
                if seen.insert(canonical.clone()) {
                    sink.push_if_collecting(canonical.clone());
                    *count += 1;
                    if matches!(
                        on_event(ScanEvent::CandidateFound {
                            path: canonical.clone(),
                            count: *count
                        }),
                        ScanControl::Cancel
                    ) {
                        return true;
                    }
                    if matches!(on_candidate(canonical), ScanVisitControl::Cancel) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn canonicalize_str(p: &Path) -> String {
    std::fs::canonicalize(p)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| p.to_string_lossy().to_string())
}

pub fn normalize_source_path(path: &str) -> String {
    if let Some(we_root) = we_workshop_root(path) {
        return canonicalize_str(Path::new(&we_root));
    }
    canonicalize_str(Path::new(path))
}

pub(super) fn we_workshop_root(path: &str) -> Option<String> {
    let (pos, marker) = find_we_marker(path)?;
    let after = path[pos + marker.len()..].trim_start_matches('/');
    let first_seg = after.split('/').next().unwrap_or("");
    if !first_seg.is_empty() && first_seg.chars().all(|c| c.is_ascii_digit()) {
        return Some(path[..pos + marker.len()].to_string());
    }
    None
}

pub fn is_wallpaper_engine_source(path: &str) -> bool {
    find_we_marker(path).is_some()
}
