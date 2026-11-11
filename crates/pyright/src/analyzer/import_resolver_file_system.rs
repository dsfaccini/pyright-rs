//! Port of `packages/pyright-internal/src/analyzer/importResolverFileSystem.ts`.

use std::cell::RefCell;

use jsrt::{FxHashMap, JsSet, P};

use crate::analyzer::import_resolver_types::ImportResolverFileSystem;
use crate::common::file_system::{Dirent, FileSystem, FsResult, ReadOnlyFileSystem, Stats};
use crate::common::path_consts::STUBS_SUFFIX;
use crate::common::path_utils::strip_file_extension;
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::{is_directory, is_file, try_realpath, try_stat};

// analyzer/importResolverFileSystem.ts:17
#[derive(jsrt::Trace)]
struct CachedDir {
    entries_by_name: FxHashMap<String, Dirent>,
    entries_array: Vec<Dirent>,
    resolvable_names: JsSet<String>,
}

// analyzer/importResolverFileSystem.ts:23
pub fn create_import_resolver_file_system(
    file_system: &'static dyn FileSystem,
) -> ImportResolverFileSystem {
    ImportResolverFileSystemImpl::new(file_system)
}

// analyzer/importResolverFileSystem.ts:27
/// Cached entries live in the arena; `invalidate_cache` drops the maps that
/// point at them.
#[derive(jsrt::Trace)]
pub struct ImportResolverFileSystemImpl {
    _cached_dir_info_for_path: RefCell<FxHashMap<Uri, P<CachedDir>>>,
    _cached_files_for_path: RefCell<FxHashMap<Uri, P<Vec<Uri>>>>,
    _cached_dir_existence_for_root: RefCell<FxHashMap<Uri, bool>>,

    _file_system: &'static dyn FileSystem,
}

impl ImportResolverFileSystemImpl {
    // analyzer/importResolverFileSystem.ts:32
    fn new(file_system: &'static dyn FileSystem) -> ImportResolverFileSystemImpl {
        ImportResolverFileSystemImpl {
            _cached_dir_info_for_path: RefCell::new(FxHashMap::default()),
            _cached_files_for_path: RefCell::new(FxHashMap::default()),
            _cached_dir_existence_for_root: RefCell::new(FxHashMap::default()),
            _file_system: file_system,
        }
    }

    // analyzer/importResolverFileSystem.ts:34
    pub fn invalidate_cache(&self) {
        self._cached_dir_info_for_path.borrow_mut().clear();
        self._cached_files_for_path.borrow_mut().clear();
        self._cached_dir_existence_for_root.borrow_mut().clear();
    }

    // analyzer/importResolverFileSystem.ts:40
    /// Never fails: a directory that cannot be read has no entries. The
    /// `ReadOnlyFileSystem` method of the same name wraps this one.
    pub fn readdir_entries_sync(&self, uri: Uri) -> &'static [Dirent] {
        &self._get_cached_dir(uri).get().entries_array
    }

    // analyzer/importResolverFileSystem.ts:44
    pub fn get_resolvable_names_in_directory(&self, dir_path: Uri) -> &'static JsSet<String> {
        &self._get_cached_dir(dir_path).get().resolvable_names
    }

    // analyzer/importResolverFileSystem.ts:48
    pub fn file_exists(&self, uri: Uri) -> bool {
        let directory = uri.get_directory();
        if directory.equals(Some(uri)) {
            // Started at root, so this can't be a file.
            return false;
        }

        let cached_dir = self._get_cached_dir(directory);
        let entry = cached_dir.entries_by_name.get(uri.file_name());
        if entry.is_some_and(|e| e.is_file) {
            return true;
        }

        if entry.is_some_and(|e| e.is_symbolic_link) {
            let real_path = try_realpath(self._file_system, uri);
            if let Some(real_path) = real_path {
                if self._file_system.exists_sync(real_path)
                    && is_file(self._file_system, real_path, false)
                {
                    return true;
                }
            }
        }

        false
    }

    // analyzer/importResolverFileSystem.ts:71
    pub fn dir_exists(&self, uri: Uri) -> bool {
        let parent = uri.get_directory();
        if parent.equals(Some(uri)) {
            // Started at root. No entries to read, so have to check ourselves.
            let cached_existence = self
                ._cached_dir_existence_for_root
                .borrow()
                .get(&uri)
                .copied();
            if let Some(cached_existence) = cached_existence {
                return cached_existence;
            }

            let exists = try_stat(self._file_system, uri).is_some_and(|s| s.is_directory);
            self._cached_dir_existence_for_root
                .borrow_mut()
                .insert(uri, exists);
            return exists;
        }

        let cached_dir = self._get_cached_dir(parent);
        let entry = cached_dir.entries_by_name.get(uri.file_name());
        if entry.is_some_and(|e| e.is_directory) {
            return true;
        }

        if entry.is_some_and(|e| e.is_symbolic_link) {
            let real_path = try_realpath(self._file_system, uri);
            if let Some(real_path) = real_path {
                if self._file_system.exists_sync(real_path)
                    && is_directory(self._file_system, real_path)
                {
                    return true;
                }
            }
        }

        false
    }

    // analyzer/importResolverFileSystem.ts:101
    pub fn get_files_in_directory(&self, dir_path: Uri) -> &'static [Uri] {
        let cached_value = self._cached_files_for_path.borrow().get(&dir_path).copied();
        if let Some(cached_value) = cached_value {
            return cached_value.get();
        }

        let entries_in_dir = self._get_cached_dir(dir_path);
        let mut files_in_dir: Vec<&Dirent> = Vec::new();

        // Add any files or symbolic links that point to files.
        entries_in_dir.entries_array.iter().for_each(|entry| {
            if entry.is_file {
                files_in_dir.push(entry);
            } else if entry.is_symbolic_link
                && try_stat(self._file_system, dir_path.combine_paths(&[&entry.name]))
                    .is_some_and(|s| s.is_file)
            {
                files_in_dir.push(entry);
            }
        });

        let new_cache_value = P::new(
            files_in_dir
                .iter()
                .map(|f| dir_path.combine_paths(&[&f.name]))
                .collect(),
        );

        self._cached_files_for_path
            .borrow_mut()
            .insert(dir_path, new_cache_value);
        new_cache_value.get()
    }

    // analyzer/importResolverFileSystem.ts:155
    fn _get_cached_dir(&self, dir_path: Uri) -> P<CachedDir> {
        let cached_value = self
            ._cached_dir_info_for_path
            .borrow()
            .get(&dir_path)
            .copied();
        if let Some(cached_value) = cached_value {
            return cached_value;
        }

        let mut entries_by_name: FxHashMap<String, Dirent> = FxHashMap::default();
        let mut resolvable_names: JsSet<String> = JsSet::default();
        let mut entries_array: Vec<Dirent> = Vec::new();

        // A read error is swallowed.
        if let Ok(entries) = self._file_system.readdir_entries_sync(dir_path) {
            entries.iter().for_each(|entry| {
                entries_by_name.insert(entry.name.clone(), entry.clone());

                let mut is_file = entry.is_file;
                let mut is_directory = entry.is_directory;
                if entry.is_symbolic_link {
                    let stat = try_stat(self._file_system, dir_path.combine_paths(&[&entry.name]));
                    is_file = stat.is_some_and(|s| s.is_file);
                    is_directory = stat.is_some_and(|s| s.is_directory);
                }

                let resolvable_name = if is_file {
                    strip_file_extension(&entry.name, /* multiDotExtension */ true)
                } else {
                    entry.name.clone()
                };

                resolvable_names.insert(resolvable_name.clone());

                if is_directory && entry.name.ends_with(STUBS_SUFFIX) {
                    resolvable_names.insert(
                        resolvable_name[..resolvable_name.len() - STUBS_SUFFIX.len()].to_string(),
                    );
                }
            });
            entries_array = entries;
        }

        let frozen = P::new(CachedDir {
            entries_by_name,
            entries_array,
            resolvable_names,
        });

        self._cached_dir_info_for_path
            .borrow_mut()
            .insert(dir_path, frozen);
        frozen
    }
}

/// `ImportResolverFileSystem` extends `Pick<FileSystem, 'existsSync' |
/// 'realCasePath' | 'getModulePath' | 'readdirEntriesSync' | 'readFileSync' |
/// 'statSync'>`. The other `ReadOnlyFileSystem` methods do not exist on the
/// TypeScript object.
impl ReadOnlyFileSystem for ImportResolverFileSystemImpl {
    // analyzer/importResolverFileSystem.ts:133
    fn exists_sync(&self, uri: Uri) -> bool {
        self._file_system.exists_sync(uri)
    }

    // analyzer/importResolverFileSystem.ts:40
    fn readdir_entries_sync(&self, uri: Uri) -> FsResult<Vec<Dirent>> {
        Ok(ImportResolverFileSystemImpl::readdir_entries_sync(self, uri).to_vec())
    }

    fn readdir_sync(&self, _uri: Uri) -> FsResult<Vec<String>> {
        unreachable!("readdirSync is not part of ImportResolverFileSystem")
    }

    // analyzer/importResolverFileSystem.ts:137
    fn read_file_sync(&self, uri: Uri) -> FsResult<Vec<u8>> {
        self._file_system.read_file_sync(uri)
    }

    // analyzer/importResolverFileSystem.ts:137
    fn read_file_text_sync(&self, uri: Uri) -> FsResult<String> {
        self._file_system.read_file_text_sync(uri)
    }

    // analyzer/importResolverFileSystem.ts:143
    fn stat_sync(&self, uri: Uri) -> FsResult<Stats> {
        self._file_system.stat_sync(uri)
    }

    fn realpath_sync(&self, _uri: Uri) -> Uri {
        unreachable!("realpathSync is not part of ImportResolverFileSystem")
    }

    // analyzer/importResolverFileSystem.ts:151
    fn get_module_path(&self) -> Uri {
        self._file_system.get_module_path()
    }

    // analyzer/importResolverFileSystem.ts:147
    fn real_case_path(&self, uri: Uri) -> Uri {
        self._file_system.real_case_path(uri)
    }

    fn is_mapped_uri(&self, _uri: Uri) -> bool {
        unreachable!("isMappedUri is not part of ImportResolverFileSystem")
    }

    fn get_original_uri(&self, _mapped_uri: Uri) -> Uri {
        unreachable!("getOriginalUri is not part of ImportResolverFileSystem")
    }

    fn get_mapped_uri(&self, _original_uri: Uri) -> Uri {
        unreachable!("getMappedUri is not part of ImportResolverFileSystem")
    }

    fn is_in_zip(&self, _uri: Uri) -> bool {
        unreachable!("isInZip is not part of ImportResolverFileSystem")
    }
}

#[cfg(test)]
mod tests {
    //! Port of the `ImportResolverFileSystem` cases in `tests/importResolverSupport.test.ts`.

    use super::*;
    use crate::tests::harness::vfs::filesystem::{TestFileSystem, file};

    fn cache_for(fs: &'static TestFileSystem) -> ImportResolverFileSystem {
        create_import_resolver_file_system(fs)
    }

    fn sorted_paths(uris: &[Uri]) -> Vec<String> {
        let mut paths: Vec<String> = uris.iter().map(|u| u.get_file_path().to_string()).collect();
        paths.sort();
        paths
    }

    #[test]
    fn readdir_entries_sync_caches_per_directory_and_is_cleared_by_invalidate_cache() {
        let fs = TestFileSystem::leak();
        fs.mkdirp_sync("/dir");
        fs.write_file_sync("/dir/a.py", "");

        let cache = cache_for(fs);
        cache.readdir_entries_sync(file("/dir"));
        cache.readdir_entries_sync(file("/dir"));
        assert_eq!(fs.call_count("readdirEntriesSync"), 1);

        cache.invalidate_cache();
        cache.readdir_entries_sync(file("/dir"));
        assert_eq!(fs.call_count("readdirEntriesSync"), 2);
    }

    #[test]
    fn file_exists_dir_exists_follow_symlinks_via_realpath() {
        let fs = TestFileSystem::leak();
        fs.mkdirp_sync("/realDir");
        fs.write_file_sync("/realFile.txt", "x");
        fs.mkdirp_sync("/links");
        fs.symlink_sync("/realFile.txt", "/links/fileLink.txt");
        fs.symlink_sync("/realDir", "/links/dirLink");

        let cache = cache_for(fs);

        assert!(cache.file_exists(file("/links/fileLink.txt")));
        assert!(!cache.file_exists(file("/links/dirLink")));

        assert!(cache.dir_exists(file("/links/dirLink")));
        assert!(!cache.dir_exists(file("/links/fileLink.txt")));
    }

    #[test]
    fn file_exists_dir_exists_return_false_for_missing_paths_and_broken_symlinks() {
        let fs = TestFileSystem::leak();
        fs.mkdirp_sync("/links");
        fs.symlink_sync("/doesNotExist", "/links/brokenLink");

        let cache = cache_for(fs);

        assert!(!cache.file_exists(file("/missingFile")));
        assert!(!cache.dir_exists(file("/missingDir")));

        assert!(!cache.file_exists(file("/links/brokenLink")));
        assert!(!cache.dir_exists(file("/links/brokenLink")));
    }

    #[test]
    fn dir_exists_caches_existence_checks_for_root() {
        let fs = TestFileSystem::leak();

        let cache = cache_for(fs);
        let root = file("/");

        assert!(cache.dir_exists(root));
        assert!(cache.dir_exists(root));

        // For the root path we should consult the filesystem once and then cache the result.
        assert_eq!(fs.call_count("statSync"), 1);
    }

    #[test]
    fn get_files_in_directory_returns_files_and_symlinks_to_files_and_caches_results() {
        let fs = TestFileSystem::leak();
        fs.mkdirp_sync("/dir");
        fs.write_file_sync("/dir/a.py", "");
        fs.write_file_sync("/realFile.txt", "x");
        fs.symlink_sync("/realFile.txt", "/dir/fileLink.txt");

        let cache = cache_for(fs);
        let dir = file("/dir");

        let files1 = cache.get_files_in_directory(dir);
        assert!(files1.iter().any(|u| u.get_file_path() == "/dir/a.py"));
        assert!(
            files1
                .iter()
                .any(|u| u.get_file_path() == "/dir/fileLink.txt")
        );

        let calls_after_first = fs.call_count("readdirEntriesSync");

        let files2 = cache.get_files_in_directory(dir);
        assert_eq!(sorted_paths(files2), sorted_paths(files1));

        // Second call should be served from cache.
        assert_eq!(fs.call_count("readdirEntriesSync"), calls_after_first);
    }

    #[test]
    fn get_files_in_directory_is_stale_until_invalidate_cache() {
        let fs = TestFileSystem::leak();
        fs.mkdirp_sync("/dir");
        fs.write_file_sync("/dir/a.py", "");

        let cache = cache_for(fs);
        let dir = file("/dir");

        assert_eq!(
            sorted_paths(cache.get_files_in_directory(dir)),
            ["/dir/a.py"]
        );

        fs.write_file_sync("/dir/b.py", "");

        // Served from cache, so it won't include newly-added files.
        assert_eq!(
            sorted_paths(cache.get_files_in_directory(dir)),
            ["/dir/a.py"]
        );

        cache.invalidate_cache();
        assert_eq!(
            sorted_paths(cache.get_files_in_directory(dir)),
            ["/dir/a.py", "/dir/b.py"]
        );
    }

    #[test]
    fn get_resolvable_names_in_directory_includes_extensionless_names_and_strips_stubs_suffix() {
        let fs = TestFileSystem::leak();
        fs.mkdirp_sync("/dir");
        fs.mkdirp_sync("/dir/pkg-stubs");
        fs.write_file_sync("/dir/foo.cpython-311-x86_64-linux-gnu.so", "");

        let cache = cache_for(fs);
        let names = cache.get_resolvable_names_in_directory(file("/dir"));

        assert!(names.contains("pkg-stubs"));
        assert!(names.contains("pkg"));
        assert!(names.contains("foo"));
    }

    #[test]
    fn get_resolvable_names_in_directory_strips_extensions_and_follows_symlinks_for_file_ness() {
        let fs = TestFileSystem::leak();
        fs.mkdirp_sync("/dir");

        fs.write_file_sync("/dir/a.py", "");
        fs.write_file_sync("/dir/b.pyi", "");
        fs.write_file_sync("/dir/c.pyc", "");
        fs.write_file_sync("/dir/foo.cpython-311-x86_64-linux-gnu.so", "");

        fs.write_file_sync("/real.pyi", "x");
        fs.symlink_sync("/real.pyi", "/dir/link.pyi");

        let cache = cache_for(fs);
        let names = cache.get_resolvable_names_in_directory(file("/dir"));

        for name in ["a", "b", "c", "foo", "link"] {
            assert!(names.contains(name), "{name}");
        }
    }

    #[test]
    fn readdir_entries_sync_failures_are_swallowed_and_cached_until_invalidate_cache() {
        let fs = TestFileSystem::leak();
        fs.mkdirp_sync("/boom");
        *fs.fail_readdir.borrow_mut() = Some("/boom".to_string());

        let cache = cache_for(fs);

        assert!(cache.readdir_entries_sync(file("/boom")).is_empty());
        assert!(cache.readdir_entries_sync(file("/boom")).is_empty());

        // Only one underlying call, and then we serve the cached empty value.
        assert_eq!(fs.call_count("readdirEntriesSync"), 1);

        cache.invalidate_cache();
        assert!(cache.readdir_entries_sync(file("/boom")).is_empty());
        assert_eq!(fs.call_count("readdirEntriesSync"), 2);
    }

    #[test]
    fn resolvable_name_of_a_file_without_a_dot_drops_its_last_character() {
        // `getFileExtension(name, true)` is `name.slice(name.indexOf('.'))`,
        // which is the last character when there is no dot.
        let fs = TestFileSystem::leak();
        fs.mkdirp_sync("/dir");
        fs.write_file_sync("/dir/README", "");

        let cache = cache_for(fs);
        let names = cache.get_resolvable_names_in_directory(file("/dir"));

        assert!(names.contains("READM"));
        assert!(!names.contains("README"));
    }
}
