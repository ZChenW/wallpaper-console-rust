use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const WORKER_MODE_ARG: &str = "__library-scan-worker";
const WORKER_REQUEST_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerSourceKind {
    Directory,
    WallpaperEngineWorkshop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanWorkerRequest {
    pub source_id: i64,
    pub source_path: PathBuf,
    pub source_kind: WorkerSourceKind,
    pub recursive: bool,
    pub snapshot_path: PathBuf,
    #[serde(default)]
    pub prior_paths: Vec<PathBuf>,
    #[serde(default)]
    pub prior_metadata: Vec<wc_core::types::WallpaperEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VersionedRequest {
    version: u32,
    request: ScanWorkerRequest,
}

#[derive(Debug)]
pub enum WorkerProtocolError {
    Io(std::io::Error),
    InvalidRequest,
    UnsupportedRequestVersion,
    InsecureRequestPermissions,
    InvalidArguments,
}

impl fmt::Display for WorkerProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => write!(formatter, "worker request I/O failed"),
            Self::InvalidRequest => write!(formatter, "worker request is invalid"),
            Self::UnsupportedRequestVersion => {
                write!(formatter, "worker request version is unsupported")
            }
            Self::InsecureRequestPermissions => {
                write!(formatter, "worker request permissions are not private")
            }
            Self::InvalidArguments => write!(formatter, "worker mode requires one request file"),
        }
    }
}

impl std::error::Error for WorkerProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for WorkerProtocolError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn write_private_worker_request(
    path: &Path,
    request: &ScanWorkerRequest,
) -> Result<(), WorkerProtocolError> {
    let payload = serde_json::to_vec(&VersionedRequest {
        version: WORKER_REQUEST_VERSION,
        request: request.clone(),
    })
    .map_err(|_| WorkerProtocolError::InvalidRequest)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options.open(path)?;
    file.write_all(&payload)?;
    file.sync_all()?;
    Ok(())
}

pub fn read_private_worker_request(path: &Path) -> Result<ScanWorkerRequest, WorkerProtocolError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(WorkerProtocolError::InvalidRequest);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(WorkerProtocolError::InsecureRequestPermissions);
        }
    }
    let payload = fs::read(path)?;
    let request: VersionedRequest =
        serde_json::from_slice(&payload).map_err(|_| WorkerProtocolError::InvalidRequest)?;
    if request.version != WORKER_REQUEST_VERSION {
        return Err(WorkerProtocolError::UnsupportedRequestVersion);
    }
    Ok(request.request)
}

pub fn worker_request_arg(args: &[String]) -> Result<Option<PathBuf>, WorkerProtocolError> {
    if args.get(1).map(String::as_str) != Some(WORKER_MODE_ARG) {
        return Ok(None);
    }
    if args.len() != 3 || args[2].is_empty() {
        return Err(WorkerProtocolError::InvalidArguments);
    }
    Ok(Some(PathBuf::from(&args[2])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_file_is_private_and_round_trips_without_process_arguments() {
        let temp = tempfile::tempdir().unwrap();
        let request_path = temp.path().join("wc-scan-test.request.json");
        let snapshot_path = temp.path().join("wc-scan-test.sqlite");
        let request = ScanWorkerRequest {
            source_id: 9,
            source_path: temp.path().join("private-source"),
            source_kind: WorkerSourceKind::Directory,
            recursive: true,
            snapshot_path,
            prior_paths: vec![temp.path().join("prior.jpg")],
            prior_metadata: Vec::new(),
        };

        write_private_worker_request(&request_path, &request).unwrap();
        let loaded = read_private_worker_request(&request_path).unwrap();
        assert_eq!(loaded, request);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&request_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn publicly_readable_request_is_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let request_path = temp.path().join("wc-scan-public.request.json");
        std::fs::write(&request_path, b"{}").unwrap();
        std::fs::set_permissions(&request_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(matches!(
            read_private_worker_request(&request_path),
            Err(WorkerProtocolError::InsecureRequestPermissions)
        ));
    }

    #[test]
    fn hidden_mode_parser_accepts_only_private_request_path_argument() {
        let args = vec![
            "wallpaper-console-rust".to_string(),
            WORKER_MODE_ARG.to_string(),
            "/run/user/1000/wc-scan-1.request.json".to_string(),
        ];
        assert_eq!(
            worker_request_arg(&args).unwrap(),
            Some(std::path::PathBuf::from(&args[2]))
        );
        assert_eq!(worker_request_arg(&args[..1]).unwrap(), None);
        assert!(worker_request_arg(&[args[0].clone(), WORKER_MODE_ARG.to_string()]).is_err());
    }
}
