//! Port of `packages/pyright-internal/src/common/fileSystem.ts`.
//!
//! Only the read side the check path uses. A method that throws in
//! TypeScript returns `Err` here; callers handle it where pyright catches.

use crate::common::uri::Uri;

// common/fileSystem.ts:17
/// `fs.Stats` as pyright reads it. `statSync` follows symbolic links.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub size: u64,
    pub mtime_ms: f64,
    pub ctime_ms: f64,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
    pub is_zip_directory: bool,
}

/// `fs.Dirent`: the entry's own type, without following a symbolic link.
#[derive(Clone, Debug)]
pub struct Dirent {
    pub name: String,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
}

impl Dirent {
    // common/fileSystem.ts:106
    /// `new VirtualDirent(name, file, parentPath)`; `parentPath` is not kept.
    pub fn virtual_dirent(name: &str, file: bool) -> Dirent {
        Dirent { name: name.to_string(), is_file: file, is_directory: !file, is_symbolic_link: false }
    }
}

/// The error a Node `fs` call throws.
#[derive(Debug)]
pub struct FsError(pub String);

impl From<std::io::Error> for FsError {
    fn from(e: std::io::Error) -> FsError {
        FsError(e.to_string())
    }
}

pub type FsResult<T> = Result<T, FsError>;

// common/fileSystem.ts:38
pub trait ReadOnlyFileSystem {
    fn exists_sync(&self, uri: Uri) -> bool;
    fn readdir_entries_sync(&self, uri: Uri) -> FsResult<Vec<Dirent>>;
    fn readdir_sync(&self, uri: Uri) -> FsResult<Vec<String>>;
    /// `readFileSync(uri)`: raw bytes.
    fn read_file_sync(&self, uri: Uri) -> FsResult<Vec<u8>>;
    /// `readFileSync(uri, 'utf8')`: invalid UTF-8 decodes to U+FFFD, as Node does.
    fn read_file_text_sync(&self, uri: Uri) -> FsResult<String> {
        Ok(String::from_utf8_lossy(&self.read_file_sync(uri)?).into_owned())
    }
    fn stat_sync(&self, uri: Uri) -> FsResult<Stats>;
    fn realpath_sync(&self, uri: Uri) -> Uri;
    fn get_module_path(&self) -> Uri;
    fn real_case_path(&self, uri: Uri) -> Uri;
    fn is_mapped_uri(&self, uri: Uri) -> bool;
    fn get_original_uri(&self, mapped_uri: Uri) -> Uri;
    fn get_mapped_uri(&self, original_uri: Uri) -> Uri;
    fn is_in_zip(&self, uri: Uri) -> bool;
}

/// vscode-jsonrpc `Disposable`: `dispose()` runs the cleanup each time it is
/// called.
pub struct Disposable {
    dispose: Box<dyn Fn()>,
}

impl Disposable {
    /// `{ dispose: () => ... }`.
    pub fn create(dispose: impl Fn() + 'static) -> Disposable {
        Disposable { dispose: Box::new(dispose) }
    }

    pub fn dispose(&self) {
        (self.dispose)()
    }
}

/// The `filter` argument of `mapDirectory`: `(originalUri: Uri, fs: FileSystem) => boolean`.
pub type MapDirectoryFilter = Box<dyn Fn(Uri, &dyn FileSystem) -> bool>;

// common/fileSystem.ts:68
/// The write side is not ported: the check never writes.
pub trait FileSystem: ReadOnlyFileSystem {
    // common/fileSystem.ts:80
    fn map_directory(&self, mapped_uri: Uri, original_uri: Uri, filter: Option<MapDirectoryFilter>) -> Disposable;
}
