use std::io::{self, Write};
#[cfg(test)]
use std::path::Path;

/// Private, securely-created raw audio kept alive for one export job.
///
/// `NamedTempFile::into_temp_path` closes the writer before FFmpeg opens the
/// path (required on Windows), while `TempPath` retains deletion ownership
/// across every normal return and panic unwind.
pub(super) struct ExportAudioTempFile {
    path: tempfile::TempPath,
    path_string: String,
}

impl ExportAudioTempFile {
    pub(super) fn from_samples(samples: &[f32]) -> io::Result<Self> {
        let mut file = tempfile::Builder::new()
            .prefix("ruvie-export-audio-")
            .suffix(".raw")
            .tempfile()?;
        for sample in samples {
            file.write_all(&sample.to_le_bytes())?;
        }
        file.flush()?;
        let path = file.into_temp_path();
        let path_string = path
            .to_str()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "system temporary directory is not valid UTF-8",
                )
            })?
            .to_string();
        Ok(Self { path, path_string })
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        self.path.as_ref()
    }

    pub(super) fn path_string(&self) -> &str {
        self.path
            .as_os_str()
            .to_str()
            .unwrap_or(self.path_string.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::ExportAudioTempFile;
    use std::fs;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn temp_audio_is_closed_for_reopen_and_removed_on_drop() {
        let temp = ExportAudioTempFile::from_samples(&[0.25, -0.5]).unwrap();
        let path = temp.path().to_path_buf();
        assert_eq!(fs::read(&path).unwrap().len(), 8);
        drop(temp);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn temp_audio_is_private_to_the_current_user() {
        use std::os::unix::fs::PermissionsExt;

        let temp = ExportAudioTempFile::from_samples(&[0.0]).unwrap();
        let mode = fs::metadata(temp.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    #[allow(
        clippy::panic,
        reason = "the test verifies TempPath cleanup during unwind"
    )]
    fn temp_audio_is_removed_during_panic_unwind() {
        let mut created_path = PathBuf::new();
        let unwind = catch_unwind(AssertUnwindSafe(|| {
            let temp = ExportAudioTempFile::from_samples(&[0.0]).unwrap();
            created_path = temp.path().to_path_buf();
            panic!("intentional unwind");
        }));
        assert!(unwind.is_err());
        assert!(!created_path.exists());
    }

    #[test]
    fn temp_audio_never_uses_or_modifies_an_output_derived_sentinel() {
        let sentinel_path =
            std::env::temp_dir().join(format!("user-output-audio-sentinel-{}.raw", Uuid::new_v4()));
        let sentinel = b"user-owned audio must survive";
        fs::write(&sentinel_path, sentinel).unwrap();

        let temp = ExportAudioTempFile::from_samples(&[1.0]).unwrap();
        assert_ne!(temp.path(), sentinel_path);
        assert_eq!(fs::read(&sentinel_path).unwrap(), sentinel);

        fs::remove_file(sentinel_path).unwrap();
    }
}
