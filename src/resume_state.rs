use crate::protocol::Rgb;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_DIRECTORY_MODE: u32 = 0o700;
const STATE_FILE_MODE: u32 = 0o600;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) trait SetAllStateStore {
    fn persist_color(&mut self, color: Rgb) -> io::Result<()>;
}

pub(crate) struct SystemSetAllStateStore;

impl SetAllStateStore for SystemSetAllStateStore {
    fn persist_color(&mut self, color: Rgb) -> io::Result<()> {
        let path = state_path_from(
            std::env::var_os("XDG_STATE_HOME").as_deref(),
            std::env::var_os("HOME").as_deref(),
        )?;
        let suffix = unique_temp_suffix();
        persist_at_with_backend(
            &path,
            &canonical_color(color),
            &suffix,
            &mut SystemAtomicStateBackend,
        )
    }
}

fn state_path_from(xdg_state_home: Option<&OsStr>, home: Option<&OsStr>) -> io::Result<PathBuf> {
    if let Some(value) = xdg_state_home.filter(|value| !value.is_empty()) {
        let root = PathBuf::from(value);
        if !root.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "XDG_STATE_HOME must be an absolute path",
            ));
        }
        return Ok(root.join("alienrgb/last-set-all-color"));
    }

    let value = home.filter(|value| !value.is_empty()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "cannot resolve resume state: neither XDG_STATE_HOME nor HOME is set",
        )
    })?;
    let root = PathBuf::from(value);
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HOME must be an absolute path",
        ));
    }
    Ok(root.join(".local/state/alienrgb/last-set-all-color"))
}

fn canonical_color(color: Rgb) -> Vec<u8> {
    format!("{:02x}{:02x}{:02x}\n", color.r, color.g, color.b).into_bytes()
}

pub fn parse_persisted_color(contents: &[u8]) -> io::Result<Rgb> {
    if contents.len() != 7 || contents[6] != b'\n' {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "resume state must contain exactly six lowercase hexadecimal digits and one newline",
        ));
    }
    if !contents[..6]
        .iter()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "resume state color must use canonical lowercase hexadecimal",
        ));
    }
    let pair = |offset| {
        std::str::from_utf8(&contents[offset..offset + 2])
            .ok()
            .and_then(|digits| u8::from_str_radix(digits, 16).ok())
            .expect("validated lowercase hexadecimal pair")
    };
    Ok(Rgb::new(pair(0), pair(2), pair(4)))
}

fn unique_temp_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{}-{timestamp}-{sequence}", std::process::id())
}

trait AtomicStateBackend {
    fn ensure_directory(&mut self, path: &Path, mode: u32) -> io::Result<()>;
    fn write_temp(&mut self, path: &Path, contents: &[u8], mode: u32) -> io::Result<()>;
    fn set_temp_permissions(&mut self, path: &Path, mode: u32) -> io::Result<()>;
    fn sync_temp(&mut self, path: &Path) -> io::Result<()>;
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    fn sync_directory(&mut self, path: &Path) -> io::Result<()>;
    fn remove_temp(&mut self, path: &Path) -> io::Result<()>;
}

fn persist_at_with_backend(
    target: &Path,
    contents: &[u8],
    temp_suffix: &str,
    backend: &mut impl AtomicStateBackend,
) -> io::Result<()> {
    let directory = target.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "resume state path has no parent directory",
        )
    })?;
    let file_name = target.file_name().and_then(OsStr::to_str).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "resume state path has no valid file name",
        )
    })?;
    let temp = directory.join(format!(".{file_name}.tmp-{temp_suffix}"));

    backend
        .ensure_directory(directory, STATE_DIRECTORY_MODE)
        .map_err(|error| contextual(error, "create secure resume state directory"))?;
    if let Err(error) = backend.write_temp(&temp, contents, STATE_FILE_MODE) {
        let _ = backend.remove_temp(&temp);
        return Err(contextual(error, "write temporary resume state"));
    }
    if let Err(error) = backend.set_temp_permissions(&temp, STATE_FILE_MODE) {
        let _ = backend.remove_temp(&temp);
        return Err(contextual(
            error,
            "set exact temporary resume state permissions",
        ));
    }
    if let Err(error) = backend.sync_temp(&temp) {
        let _ = backend.remove_temp(&temp);
        return Err(contextual(error, "sync temporary resume state"));
    }
    if let Err(error) = backend.rename(&temp, target) {
        let _ = backend.remove_temp(&temp);
        return Err(contextual(error, "atomically replace resume state"));
    }
    backend
        .sync_directory(directory)
        .map_err(|error| contextual(error, "sync resume state directory"))
}

fn contextual(error: io::Error, action: &str) -> io::Error {
    io::Error::new(error.kind(), format!("failed to {action}: {error}"))
}

struct SystemAtomicStateBackend;

impl AtomicStateBackend for SystemAtomicStateBackend {
    fn ensure_directory(&mut self, path: &Path, mode: u32) -> io::Result<()> {
        fs::create_dir_all(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }

    fn write_temp(&mut self, path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(path)?;
        file.write_all(contents)
    }

    fn set_temp_permissions(&mut self, path: &Path, mode: u32) -> io::Result<()> {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }

    fn sync_temp(&mut self, path: &Path) -> io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn sync_directory(&mut self, path: &Path) -> io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn remove_temp(&mut self, path: &Path) -> io::Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_color, parse_persisted_color, persist_at_with_backend, state_path_from,
        AtomicStateBackend,
    };
    use crate::protocol::Rgb;
    use std::ffi::OsStr;
    use std::io;
    use std::path::{Path, PathBuf};

    #[test]
    fn state_path_prefers_absolute_xdg_and_falls_back_to_absolute_home() {
        assert_eq!(
            state_path_from(Some(OsStr::new("/state")), Some(OsStr::new("/home/alice"))).unwrap(),
            PathBuf::from("/state/alienrgb/last-set-all-color")
        );
        assert_eq!(
            state_path_from(None, Some(OsStr::new("/home/alice"))).unwrap(),
            PathBuf::from("/home/alice/.local/state/alienrgb/last-set-all-color")
        );
        assert!(state_path_from(
            Some(OsStr::new("relative")),
            Some(OsStr::new("/home/alice"))
        )
        .unwrap_err()
        .to_string()
        .contains("absolute"));
        assert!(state_path_from(None, None).is_err());
    }

    #[test]
    fn persisted_color_is_canonical_lowercase_six_hex_with_one_newline() {
        assert_eq!(canonical_color(Rgb::new(0xAA, 0x00, 0xFF)), b"aa00ff\n");
        assert_eq!(
            parse_persisted_color(b"aa00ff\n").unwrap(),
            Rgb::new(0xaa, 0x00, 0xff)
        );
    }

    #[test]
    fn persisted_color_validation_rejects_noncanonical_or_extra_content() {
        for invalid in [
            b"AA00FF\n".as_slice(),
            b"#aa00ff\n",
            b"aa00ff",
            b"aa00ff\n\n",
            b"gg00ff\n",
        ] {
            assert!(parse_persisted_color(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn atomic_persistence_uses_restrictive_modes_and_rename_after_sync() {
        let mut backend = RecordingBackend::with_target(b"112233\n");
        persist_at_with_backend(
            Path::new("/state/alienrgb/last-set-all-color"),
            b"aabbcc\n",
            "test",
            &mut backend,
        )
        .unwrap();

        assert_eq!(backend.target, b"aabbcc\n");
        assert_eq!(backend.target_mode, 0o600);
        assert_eq!(
            backend.events,
            [
                "ensure_directory:700",
                "write_temp:600",
                "set_temp_permissions:600",
                "sync_temp",
                "rename_temp",
                "sync_directory"
            ]
        );
        assert!(!backend.temp_exists);
    }

    #[test]
    fn failed_atomic_rename_preserves_previous_complete_state() {
        let mut backend = RecordingBackend::with_target(b"112233\n");
        backend.fail_rename = true;
        let error = persist_at_with_backend(
            Path::new("/state/alienrgb/last-set-all-color"),
            b"aabbcc\n",
            "test",
            &mut backend,
        )
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(backend.target, b"112233\n");
        assert!(!backend.temp_exists);
        assert_eq!(backend.events.last(), Some(&"remove_temp"));
    }

    struct RecordingBackend {
        events: Vec<&'static str>,
        target: Vec<u8>,
        target_mode: u32,
        temp: Vec<u8>,
        temp_mode: u32,
        temp_exists: bool,
        fail_rename: bool,
    }

    impl RecordingBackend {
        fn with_target(contents: &[u8]) -> Self {
            Self {
                events: Vec::new(),
                target: contents.to_vec(),
                target_mode: 0o600,
                temp: Vec::new(),
                temp_mode: 0,
                temp_exists: false,
                fail_rename: false,
            }
        }
    }

    impl AtomicStateBackend for RecordingBackend {
        fn ensure_directory(&mut self, _path: &Path, mode: u32) -> io::Result<()> {
            assert_eq!(mode, 0o700);
            self.events.push("ensure_directory:700");
            Ok(())
        }

        fn write_temp(&mut self, _path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
            assert_eq!(mode, 0o600);
            self.events.push("write_temp:600");
            self.temp = contents.to_vec();
            self.temp_mode = 0;
            self.temp_exists = true;
            Ok(())
        }

        fn set_temp_permissions(&mut self, _path: &Path, mode: u32) -> io::Result<()> {
            assert_eq!(self.temp_mode, 0, "simulated restrictive umask");
            assert_eq!(mode, 0o600);
            self.events.push("set_temp_permissions:600");
            self.temp_mode = mode;
            Ok(())
        }

        fn sync_temp(&mut self, _path: &Path) -> io::Result<()> {
            self.events.push("sync_temp");
            Ok(())
        }

        fn rename(&mut self, _from: &Path, _to: &Path) -> io::Result<()> {
            self.events.push("rename_temp");
            if self.fail_rename {
                return Err(io::Error::other("forced rename failure"));
            }
            self.target = self.temp.clone();
            self.target_mode = self.temp_mode;
            self.temp_exists = false;
            Ok(())
        }

        fn sync_directory(&mut self, _path: &Path) -> io::Result<()> {
            self.events.push("sync_directory");
            Ok(())
        }

        fn remove_temp(&mut self, _path: &Path) -> io::Result<()> {
            self.events.push("remove_temp");
            self.temp_exists = false;
            Ok(())
        }
    }
}
