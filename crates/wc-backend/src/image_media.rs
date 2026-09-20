//! Bounded image decoding in the current executable's private worker mode.
//! Animated formats validate their first image, not every future animation frame.
use std::{fs::File, io::BufReader, path::Path, process::Command, time::Duration};
use wc_core::error::WcError;

const WORKER_ARG: &str = "--wc-image-preflight-worker";
const MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageStamp {
    length: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl ImageStamp {
    pub fn read(path: &str) -> Result<Self, WcError> {
        let metadata = std::fs::metadata(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                WcError::NotRegularFile(path.into())
            } else {
                WcError::Other(format!("cannot inspect image {path}: {error}"))
            }
        })?;
        if !metadata.is_file() {
            return Err(WcError::NotRegularFile(path.into()));
        }
        if metadata.len() == 0 || metadata.len() > MAX_BYTES {
            return Err(WcError::Other(
                "image must be nonempty and at most 128 MiB".into(),
            ));
        }
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified()?,
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        })
    }
    pub fn verify(&self, path: &str) -> Result<(), WcError> {
        if &Self::read(path)? != self {
            return Err(WcError::Other(format!(
                "image changed after preflight: {path}; retry apply"
            )));
        }
        Ok(())
    }
}

pub fn preflight(path: &str) -> Result<ImageStamp, WcError> {
    preflight_with_program(path, &std::env::current_exe()?, Duration::from_secs(8))
}

fn preflight_with_program(
    path: &str,
    program: &Path,
    timeout: Duration,
) -> Result<ImageStamp, WcError> {
    let stamp = ImageStamp::read(path)?;
    let output =
        crate::deadline_command::output(Command::new(program).arg(WORKER_ARG).arg(path), timeout)?;
    if !output.status.success() {
        return Err(WcError::Other(format!(
            "image preflight failed for {path}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    stamp.verify(path)?;
    Ok(stamp)
}

/// Private CLI and GUI entry point; called before configuration or window startup.
pub fn try_run_worker_mode(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some(WORKER_ARG) {
        return None;
    }
    #[cfg(unix)]
    {
        let limit = libc::rlimit {
            rlim_cur: 768 * 1024 * 1024,
            rlim_max: 768 * 1024 * 1024,
        };
        // SAFETY: limits only this dedicated worker, before decoding untrusted input.
        if unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) } != 0 {
            eprintln!("cannot establish image worker memory limit");
            return Some(1);
        }
    }
    let result = if args.len() == 3 {
        validate_image(Path::new(&args[2]))
    } else {
        Err(WcError::Other("invalid image worker arguments".into()))
    };
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    })
}

/// Real codec validation shared with tests. Production callers use the bounded worker.
pub fn validate_image(path: &Path) -> Result<(), WcError> {
    let stamp = ImageStamp::read(&path.to_string_lossy())?;
    let mut reader =
        image::ImageReader::new(BufReader::new(File::open(path)?)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(image_error)?;
    use image::ImageDecoder;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(WcError::Other(
            "image exceeds 64 megapixel preflight budget".into(),
        ));
    }
    image::DynamicImage::from_decoder(decoder).map_err(image_error)?;
    stamp.verify(&path.to_string_lossy())
}
fn image_error(error: image::ImageError) -> WcError {
    WcError::Other(format!(
        "image cannot be decoded within supported codec limits: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn stalled_image_worker_is_killed_and_reaped() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let worker = temp.path().join("worker");
        let pid = temp.path().join("pid");
        std::fs::write(
            &worker,
            format!("#!/bin/sh\necho $$ > '{}'\nsleep 30\n", pid.display()),
        )
        .unwrap();
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
        let input = temp.path().join("image.png");
        std::fs::write(&input, "data").unwrap();
        let error =
            preflight_with_program(input.to_str().unwrap(), &worker, Duration::from_millis(100))
                .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        let pid = std::fs::read_to_string(pid).unwrap();
        assert!(!Path::new(&format!("/proc/{}", pid.trim())).exists());
    }
}
