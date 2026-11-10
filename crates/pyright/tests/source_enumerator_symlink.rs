//! Port of `tests/sourceEnumeratorSymlink.test.ts`: `SourceEnumerator` does
//! not follow a symlink that resolves outside every include root.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use pyright::analyzer::source_enumerator::SourceEnumerator;
use pyright::common::case_sensitivity_detector::CaseSensitiveDetector;
use pyright::common::console::NullConsole;
use pyright::common::file_system::{
    Dirent, Disposable, FileSystem, FsError, FsResult, MapDirectoryFilter, ReadOnlyFileSystem, Stats,
};
use pyright::common::uri::Uri;
use pyright::common::uri::uri_utils::{FileSpec, get_file_spec};

fn make_uri(path: &str) -> Uri {
    Uri::file(path, &CaseSensitiveDetector, false)
}

#[derive(Clone)]
enum FsNode {
    Dir(Vec<String>),
    File,
}

/// A minimal in-memory file system with directories, files and symlinks.
#[derive(Default)]
struct MockFs {
    /// Real (symlink-resolved) nodes keyed by `uri.key`.
    nodes: HashMap<String, FsNode>,
    /// Symlink source `uri.key` to target.
    symlinks: HashMap<String, Uri>,
    /// Guards against runaway enumeration so a regression can't hang the suite.
    read_count: Cell<u32>,
}

impl MockFs {
    fn add_dir(&mut self, path: &str, children: &[&str]) {
        self.nodes.insert(make_uri(path).key().to_string(), FsNode::Dir(children.iter().map(|c| c.to_string()).collect()));
    }

    fn add_file(&mut self, path: &str) {
        self.nodes.insert(make_uri(path).key().to_string(), FsNode::File);
    }

    fn add_symlink(&mut self, path: &str, target: &str) {
        self.symlinks.insert(make_uri(path).key().to_string(), make_uri(target));
    }

    fn realpath(&self, uri: Uri) -> Uri {
        if let Some(target) = self.symlinks.get(uri.key()) {
            return self.realpath(*target);
        }

        let parent = uri.get_directory();
        if parent.key() == uri.key() {
            return uri;
        }

        let real_parent = self.realpath(parent);
        if real_parent.key() == parent.key() {
            return uri;
        }
        self.realpath(real_parent.combine_paths(&[uri.file_name()]))
    }

    fn make_dirent(&self, dir_uri: Uri, name: &str) -> Dirent {
        let requested_child = dir_uri.combine_paths(&[name]);
        let is_link = self.symlinks.contains_key(requested_child.key());
        let real_node = self.nodes.get(self.realpath(requested_child).key());
        Dirent {
            name: name.to_string(),
            is_file: !is_link && matches!(real_node, Some(FsNode::File)),
            is_directory: !is_link && matches!(real_node, Some(FsNode::Dir(_))),
            is_symbolic_link: is_link,
        }
    }
}

impl ReadOnlyFileSystem for MockFs {
    fn exists_sync(&self, uri: Uri) -> bool {
        self.nodes.contains_key(self.realpath(uri).key())
    }

    fn readdir_entries_sync(&self, uri: Uri) -> FsResult<Vec<Dirent>> {
        self.read_count.set(self.read_count.get() + 1);
        assert!(self.read_count.get() <= 10000, "Runaway enumeration: too many readdir calls");
        match self.nodes.get(self.realpath(uri).key()) {
            Some(FsNode::Dir(children)) => Ok(children.iter().map(|name| self.make_dirent(uri, name)).collect()),
            _ => Err(FsError(format!("ENOTDIR: {}", uri.to_uri_string()))),
        }
    }

    fn stat_sync(&self, uri: Uri) -> FsResult<Stats> {
        match self.nodes.get(self.realpath(uri).key()) {
            None => Err(FsError(format!("ENOENT: {}", uri.to_uri_string()))),
            Some(node) => Ok(Stats {
                is_file: matches!(node, FsNode::File),
                is_directory: matches!(node, FsNode::Dir(_)),
                ..Stats::default()
            }),
        }
    }

    /// pyright's mock throws ENOENT for a missing target; the Rust trait
    /// cannot fail, and every test resolves existing paths.
    fn realpath_sync(&self, uri: Uri) -> Uri {
        self.realpath(uri)
    }

    fn readdir_sync(&self, _uri: Uri) -> FsResult<Vec<String>> {
        unimplemented!("not in the TypeScript mock")
    }

    fn read_file_sync(&self, _uri: Uri) -> FsResult<Vec<u8>> {
        unimplemented!("not in the TypeScript mock")
    }

    fn get_module_path(&self) -> Uri {
        unimplemented!("not in the TypeScript mock")
    }

    fn real_case_path(&self, _uri: Uri) -> Uri {
        unimplemented!("not in the TypeScript mock")
    }

    fn is_mapped_uri(&self, _uri: Uri) -> bool {
        unimplemented!("not in the TypeScript mock")
    }

    fn get_original_uri(&self, _mapped_uri: Uri) -> Uri {
        unimplemented!("not in the TypeScript mock")
    }

    fn get_mapped_uri(&self, _original_uri: Uri) -> Uri {
        unimplemented!("not in the TypeScript mock")
    }

    fn is_in_zip(&self, _uri: Uri) -> bool {
        unimplemented!("not in the TypeScript mock")
    }
}

impl FileSystem for MockFs {
    fn map_directory(&self, _mapped_uri: Uri, _original_uri: Uri, _filter: Option<MapDirectoryFilter>) -> Disposable {
        unimplemented!("not in the TypeScript mock")
    }
}

fn enumerate(include_roots: &[&str], mock: MockFs) -> HashSet<String> {
    let includes: Vec<FileSpec> = include_roots.iter().map(|root| get_file_spec(make_uri(root), "**")).collect();
    let fs: &'static MockFs = Box::leak(Box::new(mock));
    let console: &'static NullConsole = Box::leak(Box::default());
    let mut enumerator = SourceEnumerator::new(&includes, Vec::new(), false, fs, console);
    let result = enumerator.enumerate(0.0);
    assert!(result.is_complete);
    result.matches.values().map(|u| u.key().to_string()).collect()
}

fn has(matches: &HashSet<String>, path: &str) -> bool {
    matches.contains(make_uri(path).key())
}

// tests/sourceEnumeratorSymlink.test.ts:159
#[test]
fn symlink_to_filesystem_root_is_not_followed() {
    let mut mock = MockFs::default();
    mock.add_dir("/workspace", &["app.py", "link"]);
    mock.add_file("/workspace/app.py");
    mock.add_symlink("/workspace/link", "/");

    // The filesystem root contains python files that must NOT be enumerated.
    mock.add_dir("/", &["secret.py", "etc"]);
    mock.add_file("/secret.py");
    mock.add_dir("/etc", &["deep.py"]);
    mock.add_file("/etc/deep.py");

    let matches = enumerate(&["/workspace"], mock);

    assert!(has(&matches, "/workspace/app.py"));
    assert!(!has(&matches, "/workspace/link/secret.py"));
    assert!(!has(&matches, "/workspace/link/etc/deep.py"));
}

// tests/sourceEnumeratorSymlink.test.ts:180
#[test]
fn symlink_to_a_sibling_directory_outside_the_workspace_is_skipped() {
    let mut mock = MockFs::default();
    mock.add_dir("/workspace", &["app.py", "esc"]);
    mock.add_file("/workspace/app.py");
    mock.add_symlink("/workspace/esc", "/other");

    mock.add_dir("/other", &["x.py"]);
    mock.add_file("/other/x.py");

    let matches = enumerate(&["/workspace"], mock);

    assert!(has(&matches, "/workspace/app.py"));
    assert!(!has(&matches, "/workspace/esc/x.py"));
}

// tests/sourceEnumeratorSymlink.test.ts:196
#[test]
fn symlink_within_the_workspace_is_followed() {
    let mut mock = MockFs::default();
    mock.add_dir("/workspace", &["sub", "real"]);
    mock.add_symlink("/workspace/sub", "/workspace/real");
    mock.add_dir("/workspace/real", &["real.py"]);
    mock.add_file("/workspace/real/real.py");

    let matches = enumerate(&["/workspace"], mock);

    // Only one of the two paths is recorded, because of the recursive-cycle guard.
    let via_real = has(&matches, "/workspace/real/real.py");
    let via_link = has(&matches, "/workspace/sub/real.py");
    assert!(via_real || via_link);
}

// tests/sourceEnumeratorSymlink.test.ts:212
#[test]
fn symlink_to_a_sibling_include_root_is_followed_multi_root() {
    let mut mock = MockFs::default();
    mock.add_dir("/root1", &["link"]);
    mock.add_symlink("/root1/link", "/root2/pkg");
    mock.add_dir("/root2", &["pkg"]);
    mock.add_dir("/root2/pkg", &["c.py"]);
    mock.add_file("/root2/pkg/c.py");

    let matches = enumerate(&["/root1", "/root2"], mock);

    assert!(has(&matches, "/root1/link/c.py"));
}

// tests/sourceEnumeratorSymlink.test.ts:227
#[test]
fn symlink_back_to_the_include_root_itself_does_not_crash_and_stays_bounded() {
    let mut mock = MockFs::default();
    mock.add_dir("/workspace", &["app.py", "self"]);
    mock.add_file("/workspace/app.py");
    // A link to the include root is caught by the recursive-cycle guard.
    mock.add_symlink("/workspace/self", "/workspace");

    let matches = enumerate(&["/workspace"], mock);

    assert!(has(&matches, "/workspace/app.py"));
}
