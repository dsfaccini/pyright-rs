//! Test double for `tests/harness/vfs/filesystem.ts`: an in-memory,
//! case-sensitive POSIX file system with directories, files and symbolic
//! links. It answers like pyright's vfs where the import tests look:
//! directory entries come in name order, `exists_sync` does not follow a
//! final symbolic link (vfs walks with `noFollow`), and `stat_sync`,
//! `readdir_entries_sync`, `read_file_sync` and `realpath_sync` follow links.
//! It records every read call in `calls` so tests can count them, the way
//! pyright's tests spy with jest.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use crate::common::case_sensitivity_detector::CaseSensitiveDetector;
use crate::common::file_system::{
    Dirent, Disposable, FileSystem, FsError, FsResult, MapDirectoryFilter, ReadOnlyFileSystem,
    Stats,
};
use crate::common::uri::Uri;

enum Node {
    Directory,
    File(Vec<u8>),
    Symlink(String),
}

pub struct TestFileSystem {
    nodes: RefCell<BTreeMap<String, Node>>,
    /// `(method, path)` for each read call, in call order.
    pub calls: RefCell<Vec<(&'static str, String)>>,
    /// `readdir_entries_sync` on this path fails.
    pub fail_readdir: RefCell<Option<String>>,
    /// `read_file_sync` and `read_file_text_sync` fail.
    pub fail_read: Cell<bool>,
}

impl Default for TestFileSystem {
    fn default() -> TestFileSystem {
        TestFileSystem::new()
    }
}

/// `UriEx.file(path)`: a case-sensitive file Uri.
pub fn file(path: &str) -> Uri {
    Uri::file(path, &CaseSensitiveDetector, false)
}

fn join(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(i) => &path[..i],
    }
}

impl TestFileSystem {
    pub fn new() -> TestFileSystem {
        let mut nodes = BTreeMap::new();
        nodes.insert("/".to_string(), Node::Directory);
        TestFileSystem {
            nodes: RefCell::new(nodes),
            calls: RefCell::new(Vec::new()),
            fail_readdir: RefCell::new(None),
            fail_read: Cell::new(false),
        }
    }

    /// A `TestFileSystem` that lives for the rest of the test thread.
    pub fn leak() -> &'static TestFileSystem {
        Box::leak(Box::new(TestFileSystem::new()))
    }

    /// The number of recorded calls to `method`.
    pub fn call_count(&self, method: &str) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|(m, _)| *m == method)
            .count()
    }

    pub fn mkdirp_sync(&self, path: &str) {
        let mut current = String::from("/");
        for component in path.split('/').filter(|c| !c.is_empty()) {
            current = join(&current, component);
            self.nodes
                .borrow_mut()
                .entry(current.clone())
                .or_insert(Node::Directory);
        }
    }

    pub fn mkdir_sync(&self, path: &str) {
        self.nodes
            .borrow_mut()
            .insert(path.to_string(), Node::Directory);
    }

    pub fn write_file_sync(&self, path: &str, content: &str) {
        self.nodes
            .borrow_mut()
            .insert(path.to_string(), Node::File(content.as_bytes().to_vec()));
    }

    /// `symlinkSync(target, path)`.
    pub fn symlink_sync(&self, target: &str, path: &str) {
        self.nodes
            .borrow_mut()
            .insert(path.to_string(), Node::Symlink(target.to_string()));
    }

    /// The real path of an existing node, following symbolic links in every
    /// component but the last unless `follow_last`.
    fn resolve(&self, path: &str, follow_last: bool, depth: u32) -> Option<String> {
        if depth > 40 {
            return None;
        }
        let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
        let mut current = String::from("/");
        for (i, component) in components.iter().enumerate() {
            let is_last = i == components.len() - 1;
            let next = match *component {
                "." => current.clone(),
                ".." => parent_of(&current).to_string(),
                _ => join(&current, component),
            };
            let target = match self.nodes.borrow().get(&next)? {
                Node::Symlink(target) if !is_last || follow_last => Some(target.clone()),
                _ => None,
            };
            current = match target {
                Some(target) => {
                    let target = if target.starts_with('/') {
                        target
                    } else {
                        join(&current, &target)
                    };
                    self.resolve(&target, true, depth + 1)?
                }
                None => next,
            };
            if !is_last && !matches!(self.nodes.borrow().get(&current), Some(Node::Directory)) {
                return None;
            }
        }
        Some(current)
    }

    fn record(&self, method: &'static str, uri: Uri) {
        self.calls
            .borrow_mut()
            .push((method, uri.get_file_path().to_string()));
    }
}

impl ReadOnlyFileSystem for TestFileSystem {
    fn exists_sync(&self, uri: Uri) -> bool {
        if uri.is_empty() {
            return false;
        }
        self.resolve(uri.get_file_path(), false, 0).is_some()
    }

    fn readdir_entries_sync(&self, uri: Uri) -> FsResult<Vec<Dirent>> {
        self.record("readdirEntriesSync", uri);
        if self.fail_readdir.borrow().as_deref() == Some(uri.get_file_path()) {
            return Err(FsError("boom".to_string()));
        }
        let Some(real) = self.resolve(uri.get_file_path(), true, 0) else {
            return Err(FsError("ENOENT".to_string()));
        };
        let nodes = self.nodes.borrow();
        if !matches!(nodes.get(&real), Some(Node::Directory)) {
            return Err(FsError("ENOTDIR".to_string()));
        }
        Ok(nodes
            .iter()
            .filter(|(path, _)| path.as_str() != "/" && parent_of(path) == real)
            .map(|(path, node)| Dirent {
                name: path[path.rfind('/').unwrap() + 1..].to_string(),
                is_file: matches!(node, Node::File(_)),
                is_directory: matches!(node, Node::Directory),
                is_symbolic_link: matches!(node, Node::Symlink(_)),
            })
            .collect())
    }

    fn readdir_sync(&self, uri: Uri) -> FsResult<Vec<String>> {
        Ok(self
            .readdir_entries_sync(uri)?
            .into_iter()
            .map(|e| e.name)
            .collect())
    }

    fn read_file_sync(&self, uri: Uri) -> FsResult<Vec<u8>> {
        self.record("readFileSync", uri);
        if self.fail_read.get() {
            return Err(FsError("read failed".to_string()));
        }
        let real = self
            .resolve(uri.get_file_path(), true, 0)
            .ok_or_else(|| FsError("ENOENT".to_string()))?;
        match self.nodes.borrow().get(&real) {
            Some(Node::File(content)) => Ok(content.clone()),
            _ => Err(FsError("EISDIR".to_string())),
        }
    }

    fn stat_sync(&self, uri: Uri) -> FsResult<Stats> {
        self.record("statSync", uri);
        let real = self
            .resolve(uri.get_file_path(), true, 0)
            .ok_or_else(|| FsError("ENOENT".to_string()))?;
        let nodes = self.nodes.borrow();
        let node = nodes
            .get(&real)
            .ok_or_else(|| FsError("ENOENT".to_string()))?;
        Ok(Stats {
            size: match node {
                Node::File(content) => content.len() as u64,
                _ => 0,
            },
            is_file: matches!(node, Node::File(_)),
            is_directory: matches!(node, Node::Directory),
            ..Stats::default()
        })
    }

    fn realpath_sync(&self, uri: Uri) -> Uri {
        self.record("realpathSync", uri);
        match self.resolve(uri.get_file_path(), true, 0) {
            Some(real) => file(&real),
            None => uri,
        }
    }

    fn get_module_path(&self) -> Uri {
        file("/")
    }

    fn real_case_path(&self, uri: Uri) -> Uri {
        uri
    }

    fn is_mapped_uri(&self, _uri: Uri) -> bool {
        false
    }

    fn get_original_uri(&self, mapped_uri: Uri) -> Uri {
        mapped_uri
    }

    fn get_mapped_uri(&self, original_uri: Uri) -> Uri {
        original_uri
    }

    fn is_in_zip(&self, _uri: Uri) -> bool {
        false
    }
}

impl FileSystem for TestFileSystem {
    fn map_directory(
        &self,
        _mapped_uri: Uri,
        _original_uri: Uri,
        _filter: Option<MapDirectoryFilter>,
    ) -> Disposable {
        panic!("Not implemented in test file system.");
    }
}
