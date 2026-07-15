//! Outcome-aware scanning for one configured wallpaper source.
//!
//! A scan only publishes entries when the whole source was enumerated. Offline,
//! incomplete, and cancelled outcomes deliberately carry no partial entries so
//! callers cannot mistake an interrupted walk for a complete empty snapshot.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
use std::path::PathBuf;

use wc_core::formats;
use wc_core::types::WallpaperEntry;

use crate::ScanControl;

/// Selects the enumeration rules for a configured source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanSourceKind {
    /// A regular directory whose recursion policy is configurable.
    Directory,
    /// A Steam Workshop root containing numeric Wallpaper Engine project dirs.
    WallpaperEngineWorkshop,
}

/// Everything needed to scan one configured source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceScanRequest {
    /// Configured source root. It need not currently be online.
    pub path: PathBuf,
    /// Enumeration rules to apply at the root.
    pub kind: ScanSourceKind,
    /// Whether a regular directory should include nested directories.
    /// Wallpaper Engine workshop enumeration intentionally ignores this flag.
    pub recursive: bool,
}

/// Monotonic counters collected while scanning one source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// Directory entries yielded by the walker.
    pub entries_visited: usize,
    /// Unique supported candidates selected for metadata inspection.
    pub candidates_found: usize,
    /// Wallpaper entries successfully built so far.
    pub entries_indexed: usize,
    /// Entries whose unchanged metadata came from the supplied prior cache.
    pub metadata_reused: usize,
}

/// Stable classification of why a source could not produce a complete snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanFailureKind {
    NotFound,
    NotDirectory,
    PermissionDenied,
    ReadDirectory,
    DirectoryEntry,
    CandidateUnavailable,
    InvalidWallpaperEngineProject,
}

/// Failure details without any partial wallpaper entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanFailure {
    pub path: PathBuf,
    pub kind: ScanFailureKind,
    pub message: String,
    pub stats: ScanStats,
}

/// Progress checkpoints, each of which may request cancellation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceScanEvent {
    SourceStarted { path: PathBuf },
    EntryVisited { path: PathBuf, stats: ScanStats },
    CandidateFound { path: PathBuf, stats: ScanStats },
}

/// A complete source snapshot. This is the only scan value that exposes entries.
#[derive(Debug, Clone)]
pub struct CompleteSourceScan {
    request: SourceScanRequest,
    entries: Vec<WallpaperEntry>,
    stats: ScanStats,
}

impl CompleteSourceScan {
    /// Return the exact source identity and enumeration settings used.
    pub fn request(&self) -> &SourceScanRequest {
        &self.request
    }

    /// Borrow all entries in the complete snapshot.
    pub fn entries(&self) -> &[WallpaperEntry] {
        &self.entries
    }

    /// Return the final scan counters.
    pub fn stats(&self) -> ScanStats {
        self.stats
    }

    /// Consume the snapshot and return its entries.
    pub fn into_entries(self) -> Vec<WallpaperEntry> {
        self.entries
    }

    /// Consume the snapshot and return both entries and counters.
    pub fn into_parts(self) -> (Vec<WallpaperEntry>, ScanStats) {
        (self.entries, self.stats)
    }
}

/// Terminal state for a single-source scan.
#[derive(Debug, Clone)]
pub enum SourceScanOutcome {
    /// The entire source was enumerated; an empty entry list is authoritative.
    Complete(CompleteSourceScan),
    /// The configured root is not currently present.
    Offline(ScanFailure),
    /// The root was present but could not be enumerated reliably.
    Incomplete(ScanFailure),
    /// The callback cancelled the scan; counters are retained, entries are not.
    Cancelled(ScanStats),
}

#[derive(Debug)]
struct WalkNode {
    path: PathBuf,
    kind: WalkNodeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkNodeKind {
    Directory,
    File,
    Other,
}

trait DirectoryReader {
    fn read_directory(
        &mut self,
        path: &Path,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<WalkNode>> + '_>>;
}

struct FsDirectoryReader;

impl DirectoryReader for FsDirectoryReader {
    fn read_directory(
        &mut self,
        path: &Path,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<WalkNode>> + '_>> {
        let entries = std::fs::read_dir(path)?;
        Ok(Box::new(entries.map(|result| {
            let entry = result?;
            let file_type = entry.file_type()?;
            let kind = if file_type.is_dir() {
                WalkNodeKind::Directory
            } else if file_type.is_file() {
                WalkNodeKind::File
            } else {
                WalkNodeKind::Other
            };
            Ok(WalkNode {
                path: entry.path(),
                kind,
            })
        })))
    }
}

/// Scan one source without a prior metadata cache.
pub fn scan_source<F>(request: &SourceScanRequest, on_event: F) -> SourceScanOutcome
where
    F: FnMut(&SourceScanEvent) -> ScanControl,
{
    scan_source_cached(request, &HashMap::new(), on_event)
}

/// Scan one source, reusing unchanged entries from `prior_metadata`.
pub fn scan_source_cached<F>(
    request: &SourceScanRequest,
    prior_metadata: &HashMap<String, WallpaperEntry>,
    on_event: F,
) -> SourceScanOutcome
where
    F: FnMut(&SourceScanEvent) -> ScanControl,
{
    scan_source_with_reader(request, prior_metadata, on_event, &mut FsDirectoryReader)
}

fn scan_source_with_reader<F, R>(
    request: &SourceScanRequest,
    prior_metadata: &HashMap<String, WallpaperEntry>,
    mut on_event: F,
    reader: &mut R,
) -> SourceScanOutcome
where
    F: FnMut(&SourceScanEvent) -> ScanControl,
    R: DirectoryReader,
{
    if matches!(
        on_event(&SourceScanEvent::SourceStarted {
            path: request.path.clone(),
        }),
        ScanControl::Cancel
    ) {
        return SourceScanOutcome::Cancelled(ScanStats::default());
    }

    let root_metadata = match std::fs::metadata(&request.path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return SourceScanOutcome::Offline(ScanFailure {
                path: request.path.clone(),
                kind: ScanFailureKind::NotFound,
                message: error.to_string(),
                stats: ScanStats::default(),
            });
        }
        Err(error) => {
            return SourceScanOutcome::Incomplete(failure_from_io(
                request.path.clone(),
                &error,
                ScanFailureKind::ReadDirectory,
                ScanStats::default(),
            ));
        }
        Ok(metadata) => metadata,
    };

    if !root_metadata.is_dir() {
        return SourceScanOutcome::Incomplete(ScanFailure {
            path: request.path.clone(),
            kind: ScanFailureKind::NotDirectory,
            message: "source root is not a directory".to_string(),
            stats: ScanStats::default(),
        });
    }

    match request.kind {
        ScanSourceKind::Directory => scan_directory(request, prior_metadata, &mut on_event, reader),
        ScanSourceKind::WallpaperEngineWorkshop => {
            scan_wallpaper_engine_workshop(request, prior_metadata, &mut on_event, reader)
        }
    }
}

fn scan_wallpaper_engine_workshop<F, R>(
    request: &SourceScanRequest,
    prior_metadata: &HashMap<String, WallpaperEntry>,
    on_event: &mut F,
    reader: &mut R,
) -> SourceScanOutcome
where
    F: FnMut(&SourceScanEvent) -> ScanControl,
    R: DirectoryReader,
{
    let mut stats = ScanStats::default();
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut manifest_free_projects = Vec::new();

    if is_single_we_project_source(&request.path) {
        let indexed = match index_we_project(&request.path, prior_metadata, on_event, &mut stats) {
            Ok(indexed) => indexed,
            Err(outcome) => return outcome,
        };
        stats.metadata_reused += usize::from(indexed.metadata_reused);
        stats.entries_indexed += 1;
        entries.push(indexed.entry);
        return SourceScanOutcome::Complete(CompleteSourceScan {
            request: request.clone(),
            entries,
            stats,
        });
    }

    let children = match reader.read_directory(&request.path) {
        Ok(children) => children,
        Err(error) => {
            return SourceScanOutcome::Incomplete(failure_from_io(
                request.path.clone(),
                &error,
                ScanFailureKind::ReadDirectory,
                stats,
            ));
        }
    };
    for child in children {
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                return SourceScanOutcome::Incomplete(failure_from_io(
                    request.path.clone(),
                    &error,
                    ScanFailureKind::DirectoryEntry,
                    stats,
                ));
            }
        };
        let WalkNode { path, kind } = child;
        stats.entries_visited += 1;
        if matches!(
            on_event(&SourceScanEvent::EntryVisited {
                path: path.clone(),
                stats,
            }),
            ScanControl::Cancel
        ) {
            return SourceScanOutcome::Cancelled(stats);
        }
        if kind != WalkNodeKind::Directory
            || should_skip_directory(&path)
            || !is_workshop_project_directory(&path)
        {
            continue;
        }

        match std::fs::symlink_metadata(path.join("project.json")) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                manifest_free_projects.push(path);
                continue;
            }
            Err(error) => {
                return SourceScanOutcome::Incomplete(failure_from_io(
                    path.join("project.json"),
                    &error,
                    ScanFailureKind::InvalidWallpaperEngineProject,
                    stats,
                ));
            }
            Ok(_) => {}
        }

        let indexed = match index_we_project(&path, prior_metadata, on_event, &mut stats) {
            Ok(indexed) => indexed,
            Err(outcome) => return outcome,
        };
        if !seen.insert(indexed.canonical_entry_path) {
            continue;
        }
        stats.metadata_reused += usize::from(indexed.metadata_reused);
        stats.entries_indexed += 1;
        entries.push(indexed.entry);
    }
    for project in manifest_free_projects {
        if let Err(outcome) = scan_manifest_free_workshop_directory(
            &project,
            prior_metadata,
            on_event,
            reader,
            &mut stats,
            &mut entries,
            &mut seen,
        ) {
            return outcome;
        }
    }

    SourceScanOutcome::Complete(CompleteSourceScan {
        request: request.clone(),
        entries,
        stats,
    })
}

fn scan_manifest_free_workshop_directory<F, R>(
    project_directory: &Path,
    prior_metadata: &HashMap<String, WallpaperEntry>,
    on_event: &mut F,
    reader: &mut R,
    stats: &mut ScanStats,
    entries: &mut Vec<WallpaperEntry>,
    seen: &mut HashSet<PathBuf>,
) -> Result<(), SourceScanOutcome>
where
    F: FnMut(&SourceScanEvent) -> ScanControl,
    R: DirectoryReader,
{
    let mut pending = vec![project_directory.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let children = reader.read_directory(&directory).map_err(|error| {
            SourceScanOutcome::Incomplete(failure_from_io(
                directory.clone(),
                &error,
                ScanFailureKind::ReadDirectory,
                *stats,
            ))
        })?;
        for child in children {
            let WalkNode { path, kind } = child.map_err(|error| {
                SourceScanOutcome::Incomplete(failure_from_io(
                    directory.clone(),
                    &error,
                    ScanFailureKind::DirectoryEntry,
                    *stats,
                ))
            })?;
            stats.entries_visited += 1;
            if matches!(
                on_event(&SourceScanEvent::EntryVisited {
                    path: path.clone(),
                    stats: *stats,
                }),
                ScanControl::Cancel
            ) {
                return Err(SourceScanOutcome::Cancelled(*stats));
            }
            if kind == WalkNodeKind::Directory {
                if !should_skip_directory(&path) {
                    pending.push(path);
                }
                continue;
            }
            if kind != WalkNodeKind::File || !is_wallpaper_candidate(&path) {
                continue;
            }

            let canonical = std::fs::canonicalize(&path).map_err(|error| {
                SourceScanOutcome::Incomplete(failure_from_io(
                    path,
                    &error,
                    ScanFailureKind::CandidateUnavailable,
                    *stats,
                ))
            })?;
            if !seen.insert(canonical.clone()) {
                continue;
            }
            stats.candidates_found += 1;
            if matches!(
                on_event(&SourceScanEvent::CandidateFound {
                    path: canonical.clone(),
                    stats: *stats,
                }),
                ScanControl::Cancel
            ) {
                return Err(SourceScanOutcome::Cancelled(*stats));
            }
            let canonical_text = canonical.to_string_lossy();
            let (entry, reused) = crate::make_entry_cached(&canonical_text, prior_metadata);
            let Some(entry) = entry else {
                return Err(SourceScanOutcome::Incomplete(ScanFailure {
                    path: canonical,
                    kind: ScanFailureKind::CandidateUnavailable,
                    message: "wallpaper candidate could not be indexed".to_string(),
                    stats: *stats,
                }));
            };
            stats.metadata_reused += usize::from(reused);
            stats.entries_indexed += 1;
            entries.push(entry);
        }
    }
    Ok(())
}

struct IndexedWeProject {
    entry: WallpaperEntry,
    canonical_entry_path: PathBuf,
    metadata_reused: bool,
}

fn index_we_project<F>(
    project_path: &Path,
    prior_metadata: &HashMap<String, WallpaperEntry>,
    on_event: &mut F,
    stats: &mut ScanStats,
) -> Result<IndexedWeProject, SourceScanOutcome>
where
    F: FnMut(&SourceScanEvent) -> ScanControl,
{
    let project_dir = std::fs::canonicalize(project_path).map_err(|error| {
        SourceScanOutcome::Incomplete(failure_from_io(
            project_path.to_path_buf(),
            &error,
            ScanFailureKind::CandidateUnavailable,
            *stats,
        ))
    })?;
    stats.candidates_found += 1;
    if matches!(
        on_event(&SourceScanEvent::CandidateFound {
            path: project_dir.clone(),
            stats: *stats,
        }),
        ScanControl::Cancel
    ) {
        return Err(SourceScanOutcome::Cancelled(*stats));
    }

    let project_json = match read_valid_we_project_json(&project_dir) {
        Ok(value) => value,
        Err(message) => {
            return Err(SourceScanOutcome::Incomplete(ScanFailure {
                path: project_dir,
                kind: ScanFailureKind::InvalidWallpaperEngineProject,
                message,
                stats: *stats,
            }));
        }
    };
    let Some(info) = crate::we_project_info_from_json(&project_dir, &project_json) else {
        return Err(SourceScanOutcome::Incomplete(ScanFailure {
            path: project_dir,
            kind: ScanFailureKind::CandidateUnavailable,
            message: "Wallpaper Engine project could not be indexed".to_string(),
            stats: *stats,
        }));
    };
    let (entry, metadata_reused) =
        crate::make_we_project_entry_from_info(project_dir.clone(), info, prior_metadata);
    let Some(entry) = entry else {
        return Err(SourceScanOutcome::Incomplete(ScanFailure {
            path: project_dir,
            kind: ScanFailureKind::CandidateUnavailable,
            message: "Wallpaper Engine project could not be indexed".to_string(),
            stats: *stats,
        }));
    };
    let canonical_entry_path =
        std::fs::canonicalize(entry.path.as_std_path()).map_err(|error| {
            SourceScanOutcome::Incomplete(failure_from_io(
                entry.path.as_std_path().to_path_buf(),
                &error,
                ScanFailureKind::CandidateUnavailable,
                *stats,
            ))
        })?;

    Ok(IndexedWeProject {
        entry,
        canonical_entry_path,
        metadata_reused,
    })
}

fn is_single_we_project_source(path: &Path) -> bool {
    match std::fs::symlink_metadata(path.join("project.json")) {
        Ok(_) => true,
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

fn is_workshop_project_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| !name.is_empty() && name.chars().all(|ch| ch.is_ascii_digit()))
}

fn read_valid_we_project_json(project_dir: &Path) -> Result<serde_json::Value, String> {
    let project_json = project_dir.join("project.json");
    let contents = std::fs::read_to_string(&project_json)
        .map_err(|error| format!("cannot read {}: {error}", project_json.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("invalid {}: {error}", project_json.display()))
}

fn scan_directory<F, R>(
    request: &SourceScanRequest,
    prior_metadata: &HashMap<String, WallpaperEntry>,
    on_event: &mut F,
    reader: &mut R,
) -> SourceScanOutcome
where
    F: FnMut(&SourceScanEvent) -> ScanControl,
    R: DirectoryReader,
{
    let mut stats = ScanStats::default();
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut pending = vec![request.path.clone()];

    while let Some(directory) = pending.pop() {
        let read_dir = match reader.read_directory(&directory) {
            Ok(read_dir) => read_dir,
            Err(error) => {
                return SourceScanOutcome::Incomplete(failure_from_io(
                    directory,
                    &error,
                    ScanFailureKind::ReadDirectory,
                    stats,
                ));
            }
        };
        for result in read_dir {
            let child = match result {
                Ok(child) => child,
                Err(error) => {
                    return SourceScanOutcome::Incomplete(failure_from_io(
                        directory.clone(),
                        &error,
                        ScanFailureKind::DirectoryEntry,
                        stats,
                    ));
                }
            };
            let WalkNode { path, kind } = child;
            stats.entries_visited += 1;
            if matches!(
                on_event(&SourceScanEvent::EntryVisited {
                    path: path.clone(),
                    stats,
                }),
                ScanControl::Cancel
            ) {
                return SourceScanOutcome::Cancelled(stats);
            }
            if kind == WalkNodeKind::Directory {
                if request.recursive && !should_skip_directory(&path) {
                    pending.push(path);
                }
                continue;
            }
            if kind != WalkNodeKind::File || !is_wallpaper_candidate(&path) {
                continue;
            }

            let canonical = match std::fs::canonicalize(&path) {
                Ok(path) => path,
                Err(error) => {
                    return SourceScanOutcome::Incomplete(failure_from_io(
                        path,
                        &error,
                        ScanFailureKind::CandidateUnavailable,
                        stats,
                    ));
                }
            };
            if !seen.insert(canonical.clone()) {
                continue;
            }
            stats.candidates_found += 1;
            if matches!(
                on_event(&SourceScanEvent::CandidateFound {
                    path: canonical.clone(),
                    stats,
                }),
                ScanControl::Cancel
            ) {
                return SourceScanOutcome::Cancelled(stats);
            }
            let canonical_text = canonical.to_string_lossy();
            let (entry, reused) = crate::make_entry_cached(&canonical_text, prior_metadata);
            let Some(entry) = entry else {
                return SourceScanOutcome::Incomplete(ScanFailure {
                    path: canonical,
                    kind: ScanFailureKind::CandidateUnavailable,
                    message: "wallpaper candidate could not be indexed".to_string(),
                    stats,
                });
            };
            stats.metadata_reused += usize::from(reused);
            stats.entries_indexed += 1;
            entries.push(entry);
        }
    }

    SourceScanOutcome::Complete(CompleteSourceScan {
        request: request.clone(),
        entries,
        stats,
    })
}

fn should_skip_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        "cache" | "caches" | "thumbnails" | "gui-thumbnails"
    )
}

fn is_wallpaper_candidate(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if formats::is_preview_filename(name) {
        return false;
    }
    formats::get_extension(&path.to_string_lossy())
        .is_some_and(|extension| formats::is_supported_extension(&extension))
}

fn failure_from_io(
    path: PathBuf,
    error: &io::Error,
    fallback_kind: ScanFailureKind,
    stats: ScanStats,
) -> ScanFailure {
    let kind = if error.kind() == io::ErrorKind::PermissionDenied {
        ScanFailureKind::PermissionDenied
    } else {
        fallback_kind
    };
    ScanFailure {
        path,
        kind,
        message: error.to_string(),
        stats,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    fn request(path: PathBuf, recursive: bool) -> SourceScanRequest {
        SourceScanRequest {
            path,
            kind: ScanSourceKind::Directory,
            recursive,
        }
    }

    fn workshop_request(path: PathBuf, recursive: bool) -> SourceScanRequest {
        SourceScanRequest {
            path,
            kind: ScanSourceKind::WallpaperEngineWorkshop,
            recursive,
        }
    }

    fn prior_project_metadata(project: &Path, resolution: &str) -> HashMap<String, WallpaperEntry> {
        let canonical_project = std::fs::canonicalize(project).unwrap();
        let mut entry = crate::make_entry(&canonical_project.to_string_lossy()).unwrap();
        let canonical_media = std::fs::canonicalize(entry.path.as_std_path()).unwrap();
        entry.resolution = resolution.to_string();
        HashMap::from([(canonical_media.to_string_lossy().to_string(), entry)])
    }

    fn complete_entries(outcome: SourceScanOutcome) -> Vec<WallpaperEntry> {
        let SourceScanOutcome::Complete(scan) = outcome else {
            panic!("expected a complete scan");
        };
        scan.into_entries()
    }

    fn entry_names(entries: &[WallpaperEntry]) -> HashSet<String> {
        entries
            .iter()
            .map(|entry| {
                entry
                    .path
                    .file_name()
                    .expect("entry must have a file name")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn complete_snapshot_retains_the_exact_scan_request() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("wall.jpg"), b"fixture").unwrap();
        let request = request(tmp.path().to_path_buf(), false);

        let outcome = scan_source(&request, |_| ScanControl::Continue);

        let SourceScanOutcome::Complete(snapshot) = outcome else {
            panic!("fixture scan must complete");
        };
        assert_eq!(snapshot.request(), &request);
    }

    #[test]
    fn missing_root_is_offline_without_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let request = request(tmp.path().join("missing"), true);

        let outcome = scan_source(&request, |_| ScanControl::Continue);

        let SourceScanOutcome::Offline(failure) = outcome else {
            panic!("missing root must be offline");
        };
        assert_eq!(failure.kind, ScanFailureKind::NotFound);
        assert_eq!(failure.path, request.path);
        assert_eq!(failure.stats, ScanStats::default());
    }

    #[test]
    fn readable_empty_directory_is_complete_empty() {
        let tmp = tempfile::tempdir().unwrap();

        let outcome = scan_source(&request(tmp.path().to_path_buf(), true), |_| {
            ScanControl::Continue
        });

        let SourceScanOutcome::Complete(scan) = outcome else {
            panic!("empty readable directory must be complete");
        };
        assert!(scan.entries().is_empty());
        assert_eq!(scan.stats(), ScanStats::default());
    }

    #[test]
    fn existing_non_directory_is_incomplete() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("wall.jpg");
        std::fs::write(&file, b"jpg").unwrap();

        let outcome = scan_source(&request(file.clone(), true), |_| ScanControl::Continue);

        let SourceScanOutcome::Incomplete(failure) = outcome else {
            panic!("a source root must be a directory");
        };
        assert_eq!(failure.kind, ScanFailureKind::NotDirectory);
        assert_eq!(failure.path, file);
    }

    #[test]
    fn non_recursive_scan_only_indexes_direct_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("direct.jpg"), b"jpg").unwrap();
        std::fs::create_dir(tmp.path().join("nested")).unwrap();
        std::fs::write(tmp.path().join("nested/inside.png"), b"png").unwrap();

        let entries = complete_entries(scan_source(
            &request(tmp.path().to_path_buf(), false),
            |_| ScanControl::Continue,
        ));

        assert_eq!(entry_names(&entries), HashSet::from(["direct.jpg".into()]));
    }

    #[test]
    fn recursive_scan_indexes_nested_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("direct.jpg"), b"jpg").unwrap();
        std::fs::create_dir(tmp.path().join("nested")).unwrap();
        std::fs::write(tmp.path().join("nested/inside.png"), b"png").unwrap();

        let entries = complete_entries(scan_source(
            &request(tmp.path().to_path_buf(), true),
            |_| ScanControl::Continue,
        ));

        assert_eq!(
            entry_names(&entries),
            HashSet::from(["direct.jpg".into(), "inside.png".into()])
        );
    }

    #[test]
    fn recursive_scan_skips_hidden_cache_and_preview_content() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hidden")).unwrap();
        std::fs::write(tmp.path().join(".hidden/secret.jpg"), b"jpg").unwrap();
        std::fs::create_dir(tmp.path().join("cache")).unwrap();
        std::fs::write(tmp.path().join("cache/generated.png"), b"png").unwrap();
        std::fs::create_dir(tmp.path().join("visible")).unwrap();
        std::fs::write(tmp.path().join("visible/wall.jpg"), b"jpg").unwrap();
        std::fs::write(tmp.path().join("preview.jpg"), b"jpg").unwrap();

        let entries = complete_entries(scan_source(
            &request(tmp.path().to_path_buf(), true),
            |_| ScanControl::Continue,
        ));

        assert_eq!(entry_names(&entries), HashSet::from(["wall.jpg".into()]));
    }

    #[test]
    fn recursive_scan_does_not_follow_directory_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let outside = tmp.path().join("outside");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(source.join("direct.jpg"), b"jpg").unwrap();
        std::fs::write(outside.join("through-link.png"), b"png").unwrap();
        std::os::unix::fs::symlink(&outside, source.join("linked")).unwrap();

        let entries = complete_entries(scan_source(&request(source, true), |_| {
            ScanControl::Continue
        }));

        assert_eq!(entry_names(&entries), HashSet::from(["direct.jpg".into()]));
    }

    #[test]
    fn callback_can_cancel_before_the_root_is_walked() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("wall.jpg"), b"jpg").unwrap();

        let outcome = scan_source(&request(tmp.path().to_path_buf(), true), |event| {
            if matches!(event, SourceScanEvent::SourceStarted { .. }) {
                ScanControl::Cancel
            } else {
                ScanControl::Continue
            }
        });

        let SourceScanOutcome::Cancelled(stats) = outcome else {
            panic!("callback must be able to cancel before walking");
        };
        assert_eq!(stats, ScanStats::default());
    }

    #[test]
    fn callback_can_cancel_mid_scan_without_exposing_partial_entries() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.jpg"), b"jpg").unwrap();
        std::fs::write(tmp.path().join("b.jpg"), b"jpg").unwrap();

        let outcome = scan_source(&request(tmp.path().to_path_buf(), true), |event| {
            if matches!(event, SourceScanEvent::CandidateFound { .. }) {
                ScanControl::Cancel
            } else {
                ScanControl::Continue
            }
        });

        let SourceScanOutcome::Cancelled(stats) = outcome else {
            panic!("cancelled scan must not expose its accumulated entries");
        };
        assert_eq!(stats.candidates_found, 1);
        assert_eq!(stats.entries_indexed, 0);
    }

    #[test]
    fn callback_can_cancel_after_one_entry_without_exposing_it() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.jpg"), b"jpg").unwrap();
        std::fs::write(tmp.path().join("b.jpg"), b"jpg").unwrap();
        let mut candidates = 0;

        let outcome = scan_source(&request(tmp.path().to_path_buf(), true), |event| {
            if matches!(event, SourceScanEvent::CandidateFound { .. }) {
                candidates += 1;
                if candidates == 2 {
                    return ScanControl::Cancel;
                }
            }
            ScanControl::Continue
        });

        let SourceScanOutcome::Cancelled(stats) = outcome else {
            panic!("cancelled scan must not expose its first entry");
        };
        assert_eq!(stats.entries_indexed, 1);
        assert_eq!(stats.candidates_found, 2);
    }

    #[test]
    fn candidate_that_disappears_during_scan_makes_source_incomplete() {
        let tmp = tempfile::tempdir().unwrap();
        let wallpaper = tmp.path().join("wall.jpg");
        std::fs::write(&wallpaper, b"jpg").unwrap();

        let outcome = scan_source(&request(tmp.path().to_path_buf(), true), |event| {
            if let SourceScanEvent::CandidateFound { path, .. } = event {
                std::fs::remove_file(path).unwrap();
            }
            ScanControl::Continue
        });

        let SourceScanOutcome::Incomplete(failure) = outcome else {
            panic!("a disappearing candidate must invalidate the source snapshot");
        };
        assert_eq!(failure.kind, ScanFailureKind::CandidateUnavailable);
    }

    #[test]
    fn overlapping_parent_and_child_scans_each_report_the_shared_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let wallpaper = child.join("wall.jpg");
        std::fs::write(&wallpaper, b"jpg").unwrap();
        let canonical = std::fs::canonicalize(&wallpaper).unwrap();

        let parent_entries = complete_entries(scan_source(
            &request(tmp.path().to_path_buf(), true),
            |_| ScanControl::Continue,
        ));
        let child_entries = complete_entries(scan_source(&request(child, true), |_| {
            ScanControl::Continue
        }));

        assert_eq!(parent_entries.len(), 1);
        assert_eq!(child_entries.len(), 1);
        assert_eq!(parent_entries[0].path.as_std_path(), canonical);
        assert_eq!(child_entries[0].path.as_std_path(), canonical);
    }

    #[test]
    fn cached_scan_reuses_unchanged_prior_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let wallpaper = tmp.path().join("wall.jpg");
        std::fs::write(&wallpaper, b"jpg").unwrap();
        let canonical = std::fs::canonicalize(&wallpaper).unwrap();
        let canonical_text = canonical.to_string_lossy().to_string();
        let mut prior = crate::make_entry(&canonical_text).unwrap();
        prior.resolution = "cached-resolution".to_string();
        let cache = HashMap::from([(canonical_text, prior)]);

        let outcome = scan_source_cached(&request(tmp.path().to_path_buf(), true), &cache, |_| {
            ScanControl::Continue
        });
        let SourceScanOutcome::Complete(scan) = outcome else {
            panic!("cached source scan must complete");
        };

        assert_eq!(scan.stats().metadata_reused, 1);
        assert_eq!(scan.entries()[0].resolution, "cached-resolution");
    }

    struct ErroringDirectoryReader {
        candidate: PathBuf,
    }

    impl DirectoryReader for ErroringDirectoryReader {
        fn read_directory(
            &mut self,
            _path: &Path,
        ) -> io::Result<Box<dyn Iterator<Item = io::Result<WalkNode>> + '_>> {
            Ok(Box::new(
                vec![
                    Ok(WalkNode {
                        path: self.candidate.clone(),
                        kind: WalkNodeKind::File,
                    }),
                    Err(io::Error::other("injected iterator failure")),
                ]
                .into_iter(),
            ))
        }
    }

    #[test]
    fn directory_iterator_error_makes_scan_incomplete_without_partial_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let candidate = tmp.path().join("wall.jpg");
        std::fs::write(&candidate, b"jpg").unwrap();
        let mut reader = ErroringDirectoryReader { candidate };

        let outcome = scan_source_with_reader(
            &request(tmp.path().to_path_buf(), true),
            &HashMap::new(),
            |_| ScanControl::Continue,
            &mut reader,
        );

        let SourceScanOutcome::Incomplete(failure) = outcome else {
            panic!("a walk error must discard all partial results");
        };
        assert_eq!(failure.kind, ScanFailureKind::DirectoryEntry);
        assert!(failure.message.contains("injected iterator failure"));
    }

    struct PanicAfterFirstIterator {
        candidate: Option<PathBuf>,
    }

    impl Iterator for PanicAfterFirstIterator {
        type Item = io::Result<WalkNode>;

        fn next(&mut self) -> Option<Self::Item> {
            if let Some(path) = self.candidate.take() {
                return Some(Ok(WalkNode {
                    path,
                    kind: WalkNodeKind::File,
                }));
            }
            panic!("directory iterator was polled after cancellation");
        }
    }

    struct CancelProbeReader {
        candidate: PathBuf,
    }

    struct PermissionDeniedReader;

    impl DirectoryReader for PermissionDeniedReader {
        fn read_directory(
            &mut self,
            _path: &Path,
        ) -> io::Result<Box<dyn Iterator<Item = io::Result<WalkNode>> + '_>> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected permission denial",
            ))
        }
    }

    #[test]
    fn unreadable_root_is_incomplete_even_when_running_as_root() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reader = PermissionDeniedReader;

        let outcome = scan_source_with_reader(
            &request(tmp.path().to_path_buf(), true),
            &HashMap::new(),
            |_| ScanControl::Continue,
            &mut reader,
        );

        let SourceScanOutcome::Incomplete(failure) = outcome else {
            panic!("permission failure must not look like a complete empty source");
        };
        assert_eq!(failure.kind, ScanFailureKind::PermissionDenied);
    }

    impl DirectoryReader for CancelProbeReader {
        fn read_directory(
            &mut self,
            _path: &Path,
        ) -> io::Result<Box<dyn Iterator<Item = io::Result<WalkNode>> + '_>> {
            Ok(Box::new(PanicAfterFirstIterator {
                candidate: Some(self.candidate.clone()),
            }))
        }
    }

    #[test]
    fn cancellation_is_checked_before_the_next_directory_item_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        let candidate = tmp.path().join("wall.jpg");
        std::fs::write(&candidate, b"jpg").unwrap();
        let mut reader = CancelProbeReader { candidate };

        let outcome = scan_source_with_reader(
            &request(tmp.path().to_path_buf(), true),
            &HashMap::new(),
            |event| {
                if matches!(event, SourceScanEvent::EntryVisited { .. }) {
                    ScanControl::Cancel
                } else {
                    ScanControl::Continue
                }
            },
            &mut reader,
        );

        let SourceScanOutcome::Cancelled(stats) = outcome else {
            panic!("scan should stop at the first visited item");
        };
        assert_eq!(stats.entries_visited, 1);
    }

    #[test]
    fn malformed_wallpaper_engine_project_makes_source_incomplete() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("123");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("project.json"), b"{not json").unwrap();

        let outcome = scan_source(&workshop_request(tmp.path().to_path_buf(), true), |_| {
            ScanControl::Continue
        });

        let SourceScanOutcome::Incomplete(failure) = outcome else {
            panic!("malformed project metadata must invalidate the complete snapshot");
        };
        assert_eq!(failure.kind, ScanFailureKind::InvalidWallpaperEngineProject);
        assert_eq!(failure.path, std::fs::canonicalize(project).unwrap());
    }

    #[test]
    fn wallpaper_engine_project_without_manifest_falls_back_to_ordinary_media() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("123");
        let nested = project.join("assets");
        std::fs::create_dir_all(&nested).unwrap();
        let wallpaper = nested.join("wall.jpg");
        std::fs::write(&wallpaper, b"jpg").unwrap();

        let entries = complete_entries(scan_source(
            &workshop_request(tmp.path().to_path_buf(), false),
            |_| ScanControl::Continue,
        ));

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].path.as_std_path(),
            std::fs::canonicalize(wallpaper).unwrap()
        );
        assert!(
            entries[0].project.is_none(),
            "manifest-free fallback media must remain an ordinary wallpaper"
        );
    }

    #[test]
    fn cancelling_manifest_free_fallback_exposes_no_partial_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("123");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("one.jpg"), b"one").unwrap();
        std::fs::write(project.join("two.jpg"), b"two").unwrap();
        let mut candidates = 0usize;

        let outcome = scan_source(
            &workshop_request(tmp.path().to_path_buf(), false),
            |event| {
                if matches!(event, SourceScanEvent::CandidateFound { .. }) {
                    candidates += 1;
                    if candidates == 2 {
                        return ScanControl::Cancel;
                    }
                }
                ScanControl::Continue
            },
        );

        let SourceScanOutcome::Cancelled(stats) = outcome else {
            panic!("cancelled fallback must not expose a complete partial snapshot");
        };
        assert_eq!(stats.candidates_found, 2);
        assert_eq!(
            stats.entries_indexed, 1,
            "the first candidate may be processed but must remain unpublished"
        );
    }

    #[test]
    fn unsupported_wallpaper_engine_projects_remain_browsable_entries() {
        let tmp = tempfile::tempdir().unwrap();
        for (id, project_type, file) in [
            ("101", "scene", "scene.json"),
            ("102", "web", "index.html"),
            ("103", "application", "app.exe"),
        ] {
            let project = tmp.path().join(id);
            std::fs::create_dir(&project).unwrap();
            std::fs::write(
                project.join("project.json"),
                format!(r#"{{"type":"{project_type}","file":"{file}"}}"#),
            )
            .unwrap();
        }

        let entries = complete_entries(scan_source(
            &workshop_request(tmp.path().to_path_buf(), false),
            |_| ScanControl::Continue,
        ));

        assert_eq!(
            entry_names(&entries),
            HashSet::from(["101".into(), "102".into(), "103".into()])
        );
        assert!(entries.iter().all(|entry| entry.project.is_some()));
    }

    #[test]
    fn wallpaper_engine_recursive_flag_does_not_scan_inside_projects() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("123");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","file":"scene.json"}"#,
        )
        .unwrap();
        std::fs::write(project.join("nested.jpg"), b"jpg").unwrap();

        let shallow = complete_entries(scan_source(
            &workshop_request(tmp.path().to_path_buf(), false),
            |_| ScanControl::Continue,
        ));
        let recursive = complete_entries(scan_source(
            &workshop_request(tmp.path().to_path_buf(), true),
            |_| ScanControl::Continue,
        ));

        assert_eq!(entry_names(&shallow), HashSet::from(["123".into()]));
        assert_eq!(entry_names(&recursive), HashSet::from(["123".into()]));
    }

    #[test]
    fn wallpaper_engine_image_project_indexes_its_media_file() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("123");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("wall.png"), b"png").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"image","file":"wall.png"}"#,
        )
        .unwrap();

        let entries = complete_entries(scan_source(
            &workshop_request(tmp.path().to_path_buf(), true),
            |_| ScanControl::Continue,
        ));

        assert_eq!(entry_names(&entries), HashSet::from(["wall.png".into()]));
        assert!(entries[0].project.is_some());
    }

    #[test]
    fn single_wallpaper_engine_project_source_indexes_its_image() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("123");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("wall.png"), b"png").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"image","file":"wall.png"}"#,
        )
        .unwrap();

        let entries = complete_entries(scan_source(&workshop_request(project, true), |_| {
            ScanControl::Continue
        }));

        assert_eq!(entry_names(&entries), HashSet::from(["wall.png".into()]));
        assert!(entries[0].project.is_some());
    }

    #[test]
    fn malformed_single_wallpaper_engine_project_source_is_incomplete() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("123");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("project.json"), b"{not json").unwrap();

        let outcome = scan_source(&workshop_request(project.clone(), false), |_| {
            ScanControl::Continue
        });

        let SourceScanOutcome::Incomplete(failure) = outcome else {
            panic!("malformed single project metadata must invalidate the source snapshot");
        };
        assert_eq!(failure.kind, ScanFailureKind::InvalidWallpaperEngineProject);
        assert_eq!(failure.path, std::fs::canonicalize(project).unwrap());
    }

    #[test]
    fn single_wallpaper_engine_project_can_cancel_before_indexing() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("123");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","file":"scene.json"}"#,
        )
        .unwrap();

        let outcome = scan_source(&workshop_request(project, true), |event| {
            if matches!(event, SourceScanEvent::CandidateFound { .. }) {
                ScanControl::Cancel
            } else {
                ScanControl::Continue
            }
        });

        let SourceScanOutcome::Cancelled(stats) = outcome else {
            panic!("single project candidate must honor cancellation");
        };
        assert_eq!(stats.candidates_found, 1);
        assert_eq!(stats.entries_indexed, 0);
    }

    #[test]
    fn disappearing_single_wallpaper_engine_media_is_incomplete() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("123");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("wall.png"), b"png").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"image","file":"wall.png"}"#,
        )
        .unwrap();

        let outcome = scan_source(&workshop_request(project, false), |event| {
            if let SourceScanEvent::CandidateFound { path, .. } = event {
                std::fs::remove_file(path.join("wall.png")).unwrap();
            }
            ScanControl::Continue
        });

        let SourceScanOutcome::Incomplete(failure) = outcome else {
            panic!("disappearing single-project media must invalidate the snapshot");
        };
        assert_eq!(failure.kind, ScanFailureKind::CandidateUnavailable);
    }

    #[test]
    fn numeric_workshop_root_without_project_json_still_enumerates_children() {
        let tmp = tempfile::tempdir().unwrap();
        let workshop = tmp.path().join("431960");
        let project = workshop.join("123");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","file":"scene.json"}"#,
        )
        .unwrap();

        let entries = complete_entries(scan_source(&workshop_request(workshop, false), |_| {
            ScanControl::Continue
        }));

        assert_eq!(entry_names(&entries), HashSet::from(["123".into()]));
    }

    #[test]
    fn non_numeric_wallpaper_engine_project_source_is_indexed() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("custom-project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","file":"scene.json"}"#,
        )
        .unwrap();

        let entries = complete_entries(scan_source(&workshop_request(project, true), |_| {
            ScanControl::Continue
        }));

        assert_eq!(
            entry_names(&entries),
            HashSet::from(["custom-project".into()])
        );
    }

    #[test]
    fn symlinked_wallpaper_engine_project_source_returns_canonical_media_path() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("real-project");
        let alias = tmp.path().join("project-alias");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("wall.png"), b"png").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"image","file":"wall.png"}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&project, &alias).unwrap();

        let entries = complete_entries(scan_source(&workshop_request(alias, false), |_| {
            ScanControl::Continue
        }));

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].path.as_std_path(),
            std::fs::canonicalize(project.join("wall.png")).unwrap()
        );
    }

    #[test]
    fn wallpaper_engine_image_scan_reuses_cached_resolution() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("image-project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("wall.png"), b"png").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"image","file":"wall.png","title":"Image"}"#,
        )
        .unwrap();
        let cache = prior_project_metadata(&project, "cached-image-resolution");

        let outcome = scan_source_cached(&workshop_request(project, true), &cache, |_| {
            ScanControl::Continue
        });
        let SourceScanOutcome::Complete(scan) = outcome else {
            panic!("cached image project scan must complete");
        };

        assert_eq!(scan.stats().metadata_reused, 1);
        assert_eq!(scan.entries()[0].resolution, "cached-image-resolution");
    }

    #[test]
    fn wallpaper_engine_video_scan_reuses_cached_resolution() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("video-project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("wall.mp4"), b"video").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"video","file":"wall.mp4","title":"Video"}"#,
        )
        .unwrap();
        let cache = prior_project_metadata(&project, "cached-video-resolution");

        let outcome = scan_source_cached(&workshop_request(project, false), &cache, |_| {
            ScanControl::Continue
        });
        let SourceScanOutcome::Complete(scan) = outcome else {
            panic!("cached video project scan must complete");
        };

        assert_eq!(scan.stats().metadata_reused, 1);
        assert_eq!(scan.entries()[0].resolution, "cached-video-resolution");
    }

    #[test]
    fn wallpaper_engine_media_change_invalidates_cached_resolution() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("video-project");
        std::fs::create_dir(&project).unwrap();
        let media = project.join("wall.mp4");
        std::fs::write(&media, b"old").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"video","file":"wall.mp4","title":"Video"}"#,
        )
        .unwrap();
        let cache = prior_project_metadata(&project, "stale-resolution");
        std::fs::write(&media, b"new-media-content").unwrap();

        let outcome = scan_source_cached(&workshop_request(project, true), &cache, |_| {
            ScanControl::Continue
        });
        let SourceScanOutcome::Complete(scan) = outcome else {
            panic!("changed video project scan must complete");
        };

        assert_eq!(scan.stats().metadata_reused, 0);
        assert_ne!(scan.entries()[0].resolution, "stale-resolution");
    }

    #[test]
    fn wallpaper_engine_project_metadata_refreshes_on_media_cache_hit() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("image-project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("wall.png"), b"png").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"image","file":"wall.png","title":"Old title"}"#,
        )
        .unwrap();
        let cache = prior_project_metadata(&project, "cached-resolution");
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"image","file":"wall.png","title":"New title"}"#,
        )
        .unwrap();

        let outcome = scan_source_cached(&workshop_request(project, false), &cache, |_| {
            ScanControl::Continue
        });
        let SourceScanOutcome::Complete(scan) = outcome else {
            panic!("metadata refresh scan must complete");
        };

        assert_eq!(scan.stats().metadata_reused, 1);
        assert_eq!(scan.entries()[0].resolution, "cached-resolution");
        assert_eq!(
            scan.entries()[0]
                .project
                .as_ref()
                .and_then(|project| project.title.as_deref()),
            Some("New title")
        );
    }
}
