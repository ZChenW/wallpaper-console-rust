//! Decode preflight and private mpv IPC evidence. Process existence is not readiness.
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Value};
use wc_core::error::WcError;

pub(crate) fn prepare_options(options: &str, path: &str) -> Result<String, WcError> {
    let result = crate::deadline_command::output(
        Command::new("ffmpeg").args([
            "-nostdin",
            "-v",
            "error",
            "-xerror",
            "-i",
            path,
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-f",
            "null",
            "-",
        ]),
        Duration::from_secs(10),
    )?;
    if !result.status.success() {
        return Err(WcError::Other(format!(
            "Media cannot be decoded; current wallpapers were kept: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    // A unique socket per launch avoids cross-output and stale-socket matches.
    // This final option reserves the control channel even if custom mpv options
    // specify another IPC server. It is internal and never saved as user options.
    let directory = socket_directory()?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| WcError::Other(e.to_string()))?
        .as_nanos();
    let socket = directory.join(format!("{}-{nonce}.sock", std::process::id()));
    Ok(format!("{options} --input-ipc-server={}", socket.display()))
}

fn socket_directory() -> Result<PathBuf, WcError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let uid = unsafe { libc::geteuid() };
    let directory = PathBuf::from(format!("/tmp/wcr-mpv-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(WcError::Other(e.to_string())),
    }
    let metadata =
        std::fs::symlink_metadata(&directory).map_err(|e| WcError::Other(e.to_string()))?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(WcError::Other(
            "Private mpv control directory is not owned exclusively by this user".into(),
        ));
    }
    Ok(directory)
}

pub(crate) fn socket_from_argv(argv: &[String]) -> Option<PathBuf> {
    let index = argv.iter().position(|arg| arg == "-o")?;
    argv.get(index + 1)?
        .split_whitespace()
        .filter_map(|arg| arg.strip_prefix("--input-ipc-server="))
        .next_back()
        .map(PathBuf::from)
}

pub(crate) fn media_ready(pid: u32, path: &str) -> bool {
    let Some(argv) = crate::process_control::read_proc_cmdline_tokens(pid as i32) else {
        return false;
    };
    let Some(socket) = socket_from_argv(&argv) else {
        return false;
    };
    query_loaded(&socket, path).unwrap_or(false)
}

fn query_loaded(socket: &Path, path: &str) -> std::io::Result<bool> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    stream.set_write_timeout(Some(Duration::from_millis(200)))?;
    for (index, name) in ["path", "idle-active", "video-params"].iter().enumerate() {
        writeln!(
            stream,
            "{}",
            json!({"command":["get_property",name],"request_id":index})
        )?;
    }
    let mut responses = [Value::Null, Value::Null, Value::Null];
    let mut received = [false; 3];
    let mut reader = BufReader::new(stream);
    // Bound both unsolicited events and response size.
    for _ in 0..20 {
        let mut line = Vec::new();
        use std::io::Read;
        let read = reader
            .by_ref()
            .take(16 * 1024)
            .read_until(b'\n', &mut line)?;
        if read == 0 || !line.ends_with(b"\n") {
            return Ok(false);
        }
        let Ok(value) = serde_json::from_slice::<Value>(&line) else {
            return Ok(false);
        };
        if let Some(index) = value["request_id"].as_u64().filter(|id| *id < 3) {
            if value["error"] != "success" {
                return Ok(false);
            }
            responses[index as usize] = value["data"].clone();
            received[index as usize] = true;
        }
        if received.iter().all(|r| *r) {
            break;
        }
    }
    Ok(responses[0].as_str() == Some(path)
        && responses[1] == false
        && responses[2]["w"].as_u64().is_some_and(|w| w > 0)
        && responses[2]["h"].as_u64().is_some_and(|h| h > 0))
}

pub(crate) fn cleanup_socket(argv: &[String]) {
    let Some(socket) = socket_from_argv(argv) else {
        return;
    };
    let Ok(directory) = socket_directory() else {
        return;
    };
    if socket.parent() == Some(directory.as_path()) {
        let _ = std::fs::remove_file(socket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_video_is_rejected_before_launch() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("skip corrupt_video_is_rejected_before_launch: ffmpeg not installed");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("bad.mp4");
        std::fs::write(&path, b"not a media file").unwrap();
        let error = prepare_options("--loop-file=inf", path.to_str().unwrap()).unwrap_err();
        assert!(
            error.to_string().contains("cannot be decoded") || error.to_string().contains("ffmpeg"),
            "{error}"
        );
    }

    #[test]
    fn live_ipc_with_no_loaded_media_is_not_ready() {
        use std::os::unix::net::UnixListener;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            for id in 0..3 {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                writeln!(
                    stream,
                    "{}",
                    json!({"request_id":id,"error":"property unavailable"})
                )
                .ok();
            }
        });
        assert!(!query_loaded(&path, "/bad.mp4").unwrap());
        worker.join().unwrap();
    }
}
