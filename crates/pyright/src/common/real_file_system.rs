//! Port of `packages/pyright-internal/src/common/realFileSystem.ts`.
//!
//! Zip and egg archives are not read: pyright's yarn `ZipOpenFS` treats a
//! `.zip`/`.egg` file with zip magic as a directory; here it stays a file.

use std::cell::Cell;
use std::time::UNIX_EPOCH;

use jsrt::JsStr;

use crate::common::case_sensitivity_detector::CaseSensitivityDetector;
use crate::common::console::ConsoleInterface;
use crate::common::file_system::{
    Dirent, Disposable, FileSystem, FsResult, MapDirectoryFilter, ReadOnlyFileSystem, Stats,
};
use crate::common::path_utils::get_root_length;
use crate::common::uri::Uri;

// common/realFileSystem.ts:37
pub fn create_from_real_file_system(
    case_sensitive_detector: &'static dyn CaseSensitivityDetector,
    console: &'static dyn ConsoleInterface,
) -> RealFileSystem {
    RealFileSystem {
        case_sensitive_detector,
        console,
    }
}

// common/realFileSystem.ts:224
pub struct RealFileSystem {
    case_sensitive_detector: &'static dyn CaseSensitivityDetector,
    console: &'static dyn ConsoleInterface,
}

fn to_ms(time: std::io::Result<std::time::SystemTime>) -> f64 {
    time.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

impl ReadOnlyFileSystem for RealFileSystem {
    // common/realFileSystem.ts:233
    fn exists_sync(&self, uri: Uri) -> bool {
        if uri.is_empty() || uri.file_data().is_none() {
            return false;
        }
        std::fs::metadata(uri.get_file_path()).is_ok()
    }

    // common/realFileSystem.ts:265
    /// Node's `fs.readdirSync` lists entries sorted by name: libuv's scandir
    /// sorts with `strcmp`, i.e. by the bytes of each name.
    fn readdir_entries_sync(&self, uri: Uri) -> FsResult<Vec<Dirent>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(uri.get_file_path())? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let name = entry.file_name();
            entries.push((
                name.as_encoded_bytes().to_vec(),
                Dirent {
                    name: name.to_string_lossy().into_owned(),
                    is_file: file_type.is_file(),
                    is_directory: file_type.is_dir(),
                    is_symbolic_link: file_type.is_symlink(),
                },
            ));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(entries.into_iter().map(|(_, dirent)| dirent).collect())
    }

    // common/realFileSystem.ts:260
    fn readdir_sync(&self, uri: Uri) -> FsResult<Vec<String>> {
        Ok(self
            .readdir_entries_sync(uri)?
            .into_iter()
            .map(|e| e.name)
            .collect())
    }

    // common/realFileSystem.ts:282
    fn read_file_sync(&self, uri: Uri) -> FsResult<Vec<u8>> {
        Ok(std::fs::read(uri.get_file_path())?)
    }

    // common/realFileSystem.ts:295
    fn stat_sync(&self, uri: Uri) -> FsResult<Stats> {
        if uri.file_data().is_none() {
            return Ok(Stats::default());
        }
        let metadata = std::fs::metadata(uri.get_file_path())?;
        Ok(Stats {
            size: metadata.len(),
            mtime_ms: to_ms(metadata.modified()),
            ctime_ms: {
                use std::os::unix::fs::MetadataExt;
                metadata.ctime() as f64 * 1000.0 + (metadata.ctime_nsec() as f64 / 1_000_000.0)
            },
            is_file: metadata.is_file(),
            is_directory: metadata.is_dir(),
            is_symbolic_link: false,
            is_zip_directory: false,
        })
    }

    // common/realFileSystem.ts:351
    fn realpath_sync(&self, uri: Uri) -> Uri {
        match std::fs::canonicalize(uri.get_file_path()) {
            Ok(path) => Uri::file(&path.to_string_lossy(), self.case_sensitive_detector, false),
            Err(_) => uri,
        }
    }

    // common/realFileSystem.ts:360
    /// The directory that holds `typeshed-fallback`: `PYRIGHT_RS_MODULE_PATH`
    /// when set, else the directory of the executable. In pyright it is the
    /// directory of the bundled `pyright-internal.js`.
    fn get_module_path(&self) -> Uri {
        let dir = match std::env::var("PYRIGHT_RS_MODULE_PATH") {
            Ok(dir) if !dir.is_empty() => dir,
            _ => std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|p| p.to_string_lossy().into_owned()))
                .unwrap_or_default(),
        };
        Uri::file(&dir, self.case_sensitive_detector, false)
    }

    // common/realFileSystem.ts:404
    fn real_case_path(&self, uri: Uri) -> Uri {
        if !self.exists_sync(uri) {
            return uri;
        }

        let real_case = match std::fs::canonicalize(uri.get_file_path()) {
            Ok(path) => path.to_string_lossy().into_owned(),
            Err(e) => {
                self.console.log(&format!(
                    "Failed to get real file system casing for {}: {e}",
                    uri.to_uri_string()
                ));
                return uri;
            }
        };

        if uri.get_file_path().js_to_lower_case() != real_case.js_to_lower_case() {
            return uri;
        }

        if get_root_length(&real_case) != get_root_length(uri.get_file_path()) {
            return uri;
        }

        Uri::file(&real_case, self.case_sensitive_detector, false)
    }

    // common/realFileSystem.ts:436
    fn is_mapped_uri(&self, _uri: Uri) -> bool {
        false
    }

    // common/realFileSystem.ts:440
    fn get_original_uri(&self, mapped_uri: Uri) -> Uri {
        mapped_uri
    }

    // common/realFileSystem.ts:444
    fn get_mapped_uri(&self, original_uri: Uri) -> Uri {
        original_uri
    }

    // common/realFileSystem.ts:457
    fn is_in_zip(&self, _uri: Uri) -> bool {
        false
    }
}

impl FileSystem for RealFileSystem {
    // common/realFileSystem.ts:448
    fn map_directory(&self, _mapped_uri: Uri, _original_uri: Uri, _filter: Option<MapDirectoryFilter>) -> Disposable {
        // Not supported at this level.
        Disposable::create(|| {
            // Do nothing.
        })
    }
}

// common/realFileSystem.ts:506
/// Only the case-sensitivity detection: the check creates no temp files.
#[derive(Default)]
pub struct RealTempFile {
    case_sensitivity: Cell<Option<bool>>,
}

impl RealTempFile {
    pub fn new() -> RealTempFile {
        RealTempFile::default()
    }

    // common/realFileSystem.ts:553
    fn is_local_file_system_case_sensitive(&self) -> bool {
        if let Some(value) = self.case_sensitivity.get() {
            return value;
        }
        let value = is_file_system_case_sensitive_internal();
        self.case_sensitivity.set(Some(value));
        value
    }
}

impl CaseSensitivityDetector for RealTempFile {
    // common/realFileSystem.ts:545
    fn is_case_sensitive(&self, uri: &str) -> bool {
        if uri.starts_with("file") {
            return self.is_local_file_system_case_sensitive();
        }

        true
    }
}

// common/realFileSystem.ts:620
/// Writes `<random>-a` to the temp directory and checks whether `<RANDOM>-A`
/// exists. Pyright does this inside its own `pyright*` temp directory.
fn is_file_system_case_sensitive_internal() -> bool {
    let dir = std::env::temp_dir();
    let mut seed = std::process::id() as u64 ^ to_ms(Ok(std::time::SystemTime::now())) as u64;
    loop {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let name = format!("{:042x}-a", seed as u128 * 0x9E37_79B9_7F4A_7C15);
        let file_path = dir.join(&name);
        let mangled_file_path = dir.join(name.to_uppercase());
        if file_path.exists() || mangled_file_path.exists() {
            continue;
        }
        if std::fs::write(&file_path, "").is_err() {
            return false;
        }
        let result = !mangled_file_path.exists();
        let _ = std::fs::remove_file(&file_path);
        return result;
    }
}
