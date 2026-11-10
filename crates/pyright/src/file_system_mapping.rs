//! Port of `packages/pyright-internal/src/fileSystemMapping.ts`.
//!
//! `createReadStream`, `readFile` and `readFileText` (streams and async I/O)
//! are not ported: the check reads with `readFileSync`.

use std::cell::RefCell;

use jsrt::{FxHashMap, JsMap, P};

use crate::common::file_system::{
    Dirent, Disposable, FileSystem, FsError, FsResult, MapDirectoryFilter, Stats,
};
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::try_stat;

// fileSystemMapping.ts:29
/// The interface has one implementation.
pub type FileSystemMapping = FileSystemMappingImpl;

// fileSystemMapping.ts:31
pub type FileSystemMappingState = FileSystemMappingStateImpl;

// fileSystemMapping.ts:35
pub fn create_file_system_mapping(real_fs: &'static dyn FileSystem) -> FileSystemMapping {
    create_file_system_mapping_state().bind(real_fs)
}

// fileSystemMapping.ts:39
pub fn create_file_system_mapping_state() -> FileSystemMappingState {
    FileSystemMappingStateImpl::new()
}

// fileSystemMapping.ts:43
/// Shared: `entryMap` and `reverseEntryMap` hold the same entry.
struct MappedEntry {
    mapped_uri: Uri,
    original_uri: Uri,
    filter: MapDirectoryFilter,
}

// fileSystemMapping.ts:49
/// `originalUriCache` is a `WeakMap` keyed by the Uri object, so it is keyed
/// by `Uri::object_id`: an equal Uri that is another object misses, as in pyright.
struct MappingData {
    entry_map: RefCell<JsMap<Uri, P<MappedEntry>>>,
    reverse_entry_map: RefCell<JsMap<Uri, P<MappedEntry>>>,
    original_uri_cache: RefCell<FxHashMap<usize, CachedOriginalUriResolution>>,
}

// fileSystemMapping.ts:55
#[derive(Clone, Copy)]
struct OriginalUriResolution {
    entry: P<MappedEntry>,
    original_uri: Uri,
}

// fileSystemMapping.ts:61
/// `noOriginalUriResolution` (fileSystemMapping.ts:60) is the first variant.
#[derive(Clone, Copy)]
enum CachedOriginalUriResolution {
    NoOriginalUriResolution,
    Resolution(OriginalUriResolution),
}

// fileSystemMapping.ts:63
pub struct FileSystemMappingStateImpl {
    _data: P<MappingData>,
}

impl FileSystemMappingStateImpl {
    fn new() -> FileSystemMappingStateImpl {
        FileSystemMappingStateImpl {
            _data: P::new(MappingData {
                entry_map: RefCell::new(JsMap::default()),
                reverse_entry_map: RefCell::new(JsMap::default()),
                original_uri_cache: RefCell::new(FxHashMap::default()),
            }),
        }
    }

    // fileSystemMapping.ts:70
    pub fn bind(&self, file_system: &'static dyn FileSystem) -> FileSystemMapping {
        FileSystemMappingImpl {
            _data: self._data,
            _real_fs: file_system,
        }
    }
}

// fileSystemMapping.ts:75
pub struct FileSystemMappingImpl {
    _data: P<MappingData>,
    _real_fs: &'static dyn FileSystem,
}

impl FileSystemMappingImpl {
    // fileSystemMapping.ts:78
    pub fn exists_sync(&self, uri: Uri) -> bool {
        if self._is_original_path(uri) {
            return false;
        }

        self._real_fs
            .exists_sync(self._get_internal_original_uri(uri))
    }

    // fileSystemMapping.ts:86
    pub fn readdir_entries_sync(&self, uri: Uri) -> FsResult<Vec<Dirent>> {
        let mut entries: JsMap<String, Dirent> = JsMap::default();

        let keys: Vec<Uri> = self._data.entry_map.borrow().keys().copied().collect();
        for key in keys {
            if key.is_child(uri) && key.get_relative_path_components(uri).len() == 1 {
                entries.insert(
                    key.file_name().to_string(),
                    Dirent::virtual_dirent(key.file_name(), false),
                );
            }
        }

        let mapped_entry = self._get_original_entry(uri);
        if let Some(mapped_entry) = mapped_entry {
            let original_uri = self._get_internal_original_uri(uri);
            for entry in self._real_fs.readdir_entries_sync(original_uri)? {
                let original_entry_uri = original_uri.combine_paths(&[&entry.name]);
                if !(mapped_entry.filter)(original_entry_uri, self._real_fs) {
                    continue;
                }

                // `target` is the entry itself or what it links to: (isFile, isDirectory).
                let target = if entry.is_file || entry.is_directory {
                    Some((entry.is_file, entry.is_directory))
                } else {
                    try_stat(self._real_fs, original_entry_uri).map(|s| (s.is_file, s.is_directory))
                };
                let Some((target_is_file, target_is_directory)) = target else {
                    continue;
                };
                if !target_is_file && !target_is_directory {
                    continue;
                }

                entries.insert(
                    entry.name.clone(),
                    Dirent::virtual_dirent(&entry.name, target_is_file),
                );
            }
        }

        if self._real_fs.exists_sync(uri) {
            let filtered_entries: Vec<Dirent> = self
                ._real_fs
                .readdir_entries_sync(uri)?
                .into_iter()
                .filter(|entry| !self._is_original_path(uri.combine_paths(&[&entry.name])))
                .collect();
            for entry in filtered_entries {
                entries.insert(entry.name.clone(), entry);
            }
        }

        Ok(entries.into_values().collect())
    }

    // fileSystemMapping.ts:125
    pub fn read_file_sync(&self, uri: Uri) -> FsResult<Vec<u8>> {
        self._real_fs
            .read_file_sync(self._get_internal_original_uri(uri))
    }

    /// `readFileSync(uri, 'utf8')`.
    pub fn read_file_text_sync(&self, uri: Uri) -> FsResult<String> {
        self._real_fs
            .read_file_text_sync(self._get_internal_original_uri(uri))
    }

    // fileSystemMapping.ts:131
    pub fn stat_sync(&self, uri: Uri) -> FsResult<Stats> {
        if self._is_original_path(uri) {
            return Err(FsError("ENOENT: path does not exist".to_string()));
        }
        self._real_fs
            .stat_sync(self._get_internal_original_uri(uri))
    }

    // fileSystemMapping.ts:138
    pub fn realpath_sync(&self, uri: Uri) -> Uri {
        if self._data.entry_map.borrow().contains_key(&uri) {
            return uri;
        }

        self._real_fs.realpath_sync(uri)
    }

    // fileSystemMapping.ts:158
    pub fn is_mapped_uri(&self, uri: Uri) -> bool {
        if self._get_original_entry(uri).is_some() {
            return true;
        }
        self._real_fs.is_mapped_uri(uri)
    }

    // fileSystemMapping.ts:165
    pub fn get_original_uri(&self, mapped_uri: Uri) -> Uri {
        self._real_fs
            .get_original_uri(self._get_internal_original_uri(mapped_uri))
    }

    // fileSystemMapping.ts:169
    pub fn get_mapped_uri(&self, original_uri: Uri) -> Uri {
        let entry = self._get_mapped_entry(original_uri);
        let Some(entry) = entry else {
            return self._real_fs.get_mapped_uri(original_uri);
        };
        let relative = entry
            .original_uri
            .get_relative_path_components(original_uri);
        let relative: Vec<&str> = relative.iter().map(String::as_str).collect();
        entry.mapped_uri.combine_paths(&relative)
    }

    // fileSystemMapping.ts:178
    pub fn map_directory(
        &self,
        mapped_uri: Uri,
        original_uri: Uri,
        filter: Option<MapDirectoryFilter>,
    ) -> Disposable {
        let entry = P::new(MappedEntry {
            original_uri,
            mapped_uri,
            filter: filter.unwrap_or_else(|| Box::new(|_, _| true)),
        });
        self._data.entry_map.borrow_mut().insert(mapped_uri, entry);
        self._data
            .reverse_entry_map
            .borrow_mut()
            .insert(original_uri, entry);
        *self._data.original_uri_cache.borrow_mut() = FxHashMap::default();
        let data = self._data;
        Disposable::create(move || {
            data.entry_map.borrow_mut().shift_remove(&mapped_uri);
            data.reverse_entry_map
                .borrow_mut()
                .shift_remove(&original_uri);
            *data.original_uri_cache.borrow_mut() = FxHashMap::default();
        })
    }

    // fileSystemMapping.ts:192
    fn _find_closest_match(
        &self,
        uri: Uri,
        map: &RefCell<JsMap<Uri, P<MappedEntry>>>,
    ) -> Option<P<MappedEntry>> {
        let mut uri = uri;
        loop {
            let entry = map.borrow().get(&uri).copied();
            if let Some(entry) = entry {
                return Some(entry);
            }

            let parent = uri.get_directory();
            if parent.equals(Some(uri)) {
                return None;
            }

            uri = parent;
        }
    }

    // fileSystemMapping.ts:208
    fn _get_original_entry(&self, uri: Uri) -> Option<P<MappedEntry>> {
        self._find_closest_match(uri, &self._data.entry_map)
    }

    // fileSystemMapping.ts:212
    fn _get_internal_original_uri(&self, uri: Uri) -> Uri {
        let cached = self._data.original_uri_cache.borrow().get(&uri.object_id()).copied();
        let resolution = match cached {
            Some(resolution) => resolution,
            None => {
                let entry = self._get_original_entry(uri);
                let Some(entry) = entry else {
                    self._data
                        .original_uri_cache
                        .borrow_mut()
                        .insert(uri.object_id(), CachedOriginalUriResolution::NoOriginalUriResolution);
                    return uri;
                };

                let relative = entry.mapped_uri.get_relative_path_components(uri);
                let relative: Vec<&str> = relative.iter().map(String::as_str).collect();
                let resolution = CachedOriginalUriResolution::Resolution(OriginalUriResolution {
                    entry,
                    original_uri: entry.original_uri.combine_paths(&relative),
                });
                self._data
                    .original_uri_cache
                    .borrow_mut()
                    .insert(uri.object_id(), resolution);
                resolution
            }
        };

        let CachedOriginalUriResolution::Resolution(resolution) = resolution else {
            return uri;
        };

        if (resolution.entry.filter)(resolution.original_uri, self._real_fs) {
            return resolution.original_uri;
        }

        uri
    }

    // fileSystemMapping.ts:237
    fn _get_mapped_entry(&self, uri: Uri) -> Option<P<MappedEntry>> {
        let reverse_match = self._find_closest_match(uri, &self._data.reverse_entry_map);
        if let Some(reverse_match) = reverse_match {
            if (reverse_match.filter)(uri, self._real_fs) {
                return Some(reverse_match);
            }
        }
        None
    }

    // fileSystemMapping.ts:245
    fn _is_original_path(&self, uri: Uri) -> bool {
        self._get_mapped_entry(uri).is_some()
    }
}

#[cfg(test)]
mod tests {
    //! Port of the synchronous cases in `tests/fileSystemMapping.test.ts`.

    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;
    use crate::common::case_sensitivity_detector::CaseSensitivityDetector;
    use crate::common::file_system::ReadOnlyFileSystem;
    use crate::pyright_file_system::PyrightFileSystem;
    use crate::tests::harness::vfs::filesystem::{TestFileSystem, file};

    /// `new TestFileSystem(false, { files })`.
    fn fs_with(files: &[(Uri, &str)]) -> &'static TestFileSystem {
        let fs = TestFileSystem::leak();
        for (uri, content) in files {
            fs.mkdirp_sync(uri.get_directory().get_file_path());
            fs.write_file_sync(uri.get_file_path(), content);
        }
        fs
    }

    fn text(mapping: &FileSystemMapping, uri: Uri) -> String {
        mapping.read_file_text_sync(uri).unwrap()
    }

    fn names(entries: &[Dirent]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    type FilterCalls = Rc<RefCell<Vec<(Uri, *const ())>>>;

    fn recording_filter(
        calls: &FilterCalls,
        result: impl Fn(Uri) -> bool + 'static,
    ) -> Option<MapDirectoryFilter> {
        let calls = calls.clone();
        Some(Box::new(move |uri, file_system| {
            calls
                .borrow_mut()
                .push((uri, std::ptr::from_ref(file_system).cast::<()>()));
            result(uri)
        }))
    }

    fn called_with(calls: &FilterCalls, uri: Uri, fs: &'static TestFileSystem) -> bool {
        calls
            .borrow()
            .iter()
            .any(|(u, f)| u.equals(Some(uri)) && std::ptr::addr_eq(*f, fs as *const TestFileSystem))
    }

    #[test]
    fn mapping_state_shares_registrations_while_each_view_uses_its_own_downstream_filesystem() {
        let public_root = file("/public/pkg");
        let original_root = file("/original/pkg");
        let public_file = public_root.combine_paths(&["file.py"]);
        let original_file = original_root.combine_paths(&["file.py"]);
        let first_fs = fs_with(&[(original_file, "first")]);
        let second_fs = fs_with(&[(original_file, "second")]);
        let state = create_file_system_mapping_state();
        let first = state.bind(first_fs);
        let second = state.bind(second_fs);
        let filter_calls: FilterCalls = Rc::default();
        let mapping = first.map_directory(
            public_root,
            original_root,
            recording_filter(&filter_calls, |_| true),
        );

        assert!(first.is_mapped_uri(public_file));
        assert!(second.is_mapped_uri(public_file));
        assert_eq!(text(&first, public_file), "first");
        assert_eq!(text(&second, public_file), "second");
        assert!(called_with(&filter_calls, original_file, first_fs));
        assert!(called_with(&filter_calls, original_file, second_fs));

        mapping.dispose();
        assert!(!first.is_mapped_uri(public_file));
        assert!(!second.is_mapped_uri(public_file));
    }

    #[test]
    fn mapping_state_invalidates_shared_cached_misses_when_a_mapping_is_added() {
        let public_root = file("/public/pkg");
        let original_root = file("/original/pkg");
        let public_file = public_root.combine_paths(&["file.py"]);
        let original_file = original_root.combine_paths(&["file.py"]);
        let fs = fs_with(&[(public_file, "public"), (original_file, "original")]);
        let state = create_file_system_mapping_state();
        let first = state.bind(fs);
        let second = state.bind(fs);

        assert_eq!(text(&first, public_file), "public");
        assert_eq!(text(&second, public_file), "public");

        first.map_directory(public_root, original_root, None);

        assert_eq!(text(&second, public_file), "original");
        assert_eq!(text(&first, public_file), "original");
    }

    #[test]
    fn mapping_state_preserves_replacement_and_stale_handle_disposal_across_bound_views() {
        let public_root = file("/public/pkg");
        let first_original = file("/original/first");
        let second_original = file("/original/second");
        let public_file = public_root.combine_paths(&["file.py"]);
        let fs = fs_with(&[
            (public_file, "public"),
            (first_original.combine_paths(&["file.py"]), "first"),
            (second_original.combine_paths(&["file.py"]), "second"),
        ]);
        let state = create_file_system_mapping_state();
        let first = state.bind(fs);
        let second = state.bind(fs);
        let stale = first.map_directory(public_root, first_original, None);
        assert_eq!(text(&second, public_file), "first");

        second.map_directory(public_root, second_original, None);
        assert_eq!(text(&first, public_file), "second");

        stale.dispose();
        assert!(!first.is_mapped_uri(public_file));
        assert!(!second.is_mapped_uri(public_file));
        assert_eq!(text(&second, public_file), "public");
    }

    #[test]
    fn independent_mapping_states_remain_isolated_over_the_same_downstream_filesystem() {
        let first_public = file("/public/first");
        let second_public = file("/public/second");
        let first_original = file("/original/first");
        let second_original = file("/original/second");
        let fs = fs_with(&[
            (first_original.combine_paths(&["file.py"]), "first"),
            (second_original.combine_paths(&["file.py"]), "second"),
        ]);
        let first = create_file_system_mapping_state().bind(fs);
        let second = create_file_system_mapping_state().bind(fs);
        first.map_directory(first_public, first_original, None);
        second.map_directory(second_public, second_original, None);

        assert!(first.is_mapped_uri(first_public));
        assert!(!first.is_mapped_uri(second_public));
        assert!(!second.is_mapped_uri(first_public));
        assert!(second.is_mapped_uri(second_public));
    }

    #[test]
    fn compatibility_mapping_factory_creates_isolated_state_for_every_call() {
        let public_root = file("/public/pkg");
        let original_root = file("/original/pkg");
        let public_file = public_root.combine_paths(&["file.py"]);
        let fs = fs_with(&[(original_root.combine_paths(&["file.py"]), "content")]);
        let first = create_file_system_mapping(fs);
        let second = create_file_system_mapping(fs);

        first.map_directory(public_root, original_root, None);

        assert!(first.is_mapped_uri(public_file));
        assert!(!second.is_mapped_uri(public_file));
    }

    #[test]
    fn file_system_mapping_translates_children_filters_originals_and_disposes_mappings() {
        let public_root = file("/public/pkg");
        let original_root = file("/original/pkg");
        let public_file = public_root.combine_paths(&["file.py"]);
        let original_file = original_root.combine_paths(&["file.py"]);
        let denied_public = public_root.combine_paths(&["denied.py"]);
        let denied_original = original_root.combine_paths(&["denied.py"]);
        let fs = fs_with(&[
            (original_file, "original"),
            (denied_original, "denied original"),
            (denied_public, "public fallback"),
        ]);
        let mapping = create_file_system_mapping(fs);
        let filter_calls: FilterCalls = Rc::default();
        let disposable = mapping.map_directory(
            public_root,
            original_root,
            recording_filter(&filter_calls, move |uri| !uri.equals(Some(denied_original))),
        );

        let equal_public_file = file(public_file.get_file_path());
        assert!(mapping.is_mapped_uri(equal_public_file));
        assert!(
            mapping
                .get_original_uri(equal_public_file)
                .equals(Some(original_file))
        );
        assert!(
            mapping
                .get_mapped_uri(file(original_file.get_file_path()))
                .equals(Some(public_file))
        );
        assert_eq!(text(&mapping, public_file), "original");
        assert!(!mapping.exists_sync(original_file));
        assert_eq!(
            mapping.stat_sync(original_file).unwrap_err().0,
            "ENOENT: path does not exist"
        );

        assert!(mapping.is_mapped_uri(denied_public));
        assert!(
            mapping
                .get_mapped_uri(denied_original)
                .equals(Some(denied_original))
        );
        assert!(mapping.exists_sync(denied_original));
        assert_eq!(text(&mapping, denied_public), "public fallback");
        assert!(called_with(&filter_calls, original_file, fs));
        assert!(called_with(&filter_calls, denied_original, fs));

        disposable.dispose();
        assert!(!mapping.is_mapped_uri(public_file));
        assert!(mapping.exists_sync(original_file));
    }

    struct CaseInsensitiveDetector;

    impl CaseSensitivityDetector for CaseInsensitiveDetector {
        fn is_case_sensitive(&self, _uri: &str) -> bool {
            false
        }
    }

    #[test]
    fn file_system_mapping_follows_uri_case_sensitivity_semantics() {
        let insensitive = |path: &str| Uri::file(path, &CaseInsensitiveDetector, false);
        let fs = TestFileSystem::leak();
        let mapping = create_file_system_mapping(fs);
        let sensitive_mapped = file("/Public/Sensitive");
        let sensitive_original = file("/Original/Sensitive");
        let insensitive_mapped = insensitive("/Public/Insensitive");
        let insensitive_original = insensitive("/Original/Insensitive");
        mapping.map_directory(sensitive_mapped, sensitive_original, None);
        mapping.map_directory(insensitive_mapped, insensitive_original, None);

        let sensitive_mismatch = file("/public/sensitive/file.py");
        let insensitive_alias = insensitive("/PUBLIC/INSENSITIVE/file.py");

        assert_eq!(
            mapping.get_original_uri(sensitive_mismatch),
            sensitive_mismatch
        );
        assert!(
            mapping
                .get_original_uri(insensitive_alias)
                .equals(Some(insensitive("/Original/Insensitive/file.py")))
        );
    }

    #[test]
    fn file_system_mapping_uses_closest_parents_and_exact_root_realpath_behavior() {
        let public_root = file("/public/pkg");
        let original_root = file("/original/pkg");
        let nested_public = public_root.combine_paths(&["nested"]);
        let nested_original = file("/nested-source");
        let nested_file = nested_public.combine_paths(&["file.py"]);
        let sibling_file = public_root.combine_paths(&["nestedish", "file.py"]);
        let fs = fs_with(&[
            (nested_original.combine_paths(&["file.py"]), "nested"),
            (
                original_root.combine_paths(&["nested", "file.py"]),
                "parent nested",
            ),
            (
                original_root.combine_paths(&["nestedish", "file.py"]),
                "sibling",
            ),
        ]);
        let mapping = create_file_system_mapping(fs);
        mapping.map_directory(public_root, original_root, None);
        assert_eq!(text(&mapping, nested_file), "parent nested");

        let nested = mapping.map_directory(nested_public, nested_original, None);

        assert_eq!(text(&mapping, nested_file), "nested");
        assert_eq!(text(&mapping, sibling_file), "sibling");
        assert_eq!(mapping.realpath_sync(public_root), public_root);
        assert_eq!(mapping.realpath_sync(nested_file), nested_file);
        assert_eq!(fs.call_count("realpathSync"), 1);
        assert_eq!(
            fs.calls
                .borrow()
                .iter()
                .find(|(m, _)| *m == "realpathSync")
                .unwrap()
                .1,
            nested_file.get_file_path()
        );

        nested.dispose();
        assert_eq!(text(&mapping, nested_file), "parent nested");
    }

    #[test]
    fn file_system_mapping_preserves_stale_and_independent_disposable_behavior() {
        let public_root = file("/public/pkg");
        let first_original = file("/original/first");
        let second_original = file("/original/second");
        let other_public = file("/public/other");
        let other_original = file("/original/other");
        let public_file = public_root.combine_paths(&["file.py"]);
        let fs = fs_with(&[
            (public_file, "public"),
            (first_original.combine_paths(&["file.py"]), "first"),
            (second_original.combine_paths(&["file.py"]), "second"),
            (other_original.combine_paths(&["file.py"]), "other"),
        ]);
        let mapping = create_file_system_mapping(fs);
        let stale = mapping.map_directory(public_root, first_original, None);
        assert_eq!(text(&mapping, public_file), "first");
        mapping.map_directory(public_root, second_original, None);
        assert_eq!(text(&mapping, public_file), "second");
        let other = mapping.map_directory(other_public, other_original, None);

        stale.dispose();
        assert!(!mapping.is_mapped_uri(public_file));
        assert_eq!(text(&mapping, public_file), "public");
        assert_eq!(
            text(&mapping, other_public.combine_paths(&["file.py"])),
            "other"
        );
        other.dispose();
        assert!(mapping.exists_sync(other_original.combine_paths(&["file.py"])));
    }

    #[test]
    fn file_system_mapping_synthesizes_and_merges_classified_directory_entries() {
        let public_parent = file("/public");
        let public_root = public_parent.combine_paths(&["pkg"]);
        let original_root = file("/original/pkg");
        let linked_target = file("/targets/linked.pyi");
        let linked_path = original_root.combine_paths(&["linked.pyi"]);
        let missing_path = original_root.combine_paths(&["missing.pyi"]);
        let fs = TestFileSystem::leak();
        fs.mkdirp_sync(original_root.get_file_path());
        fs.mkdirp_sync(public_root.get_file_path());
        fs.mkdirp_sync(linked_target.get_directory().get_file_path());
        fs.mkdirp_sync(original_root.combine_paths(&["subdir"]).get_file_path());
        fs.write_file_sync(linked_target.get_file_path(), "linked");
        fs.write_file_sync(
            original_root.combine_paths(&["dup.pyi"]).get_file_path(),
            "mapped duplicate",
        );
        fs.write_file_sync(
            public_root.combine_paths(&["dup.pyi"]).get_file_path(),
            "real duplicate",
        );
        fs.write_file_sync(
            public_root.combine_paths(&["real.py"]).get_file_path(),
            "real",
        );
        fs.symlink_sync(linked_target.get_file_path(), linked_path.get_file_path());
        fs.symlink_sync("/targets/missing.pyi", missing_path.get_file_path());
        let mapping = create_file_system_mapping(fs);
        mapping.map_directory(
            public_root,
            original_root,
            Some(Box::new(move |uri, _| {
                uri.equals(Some(original_root))
                    || uri.file_name() == "subdir"
                    || uri.file_name().ends_with(".pyi")
            })),
        );

        let parent_entries = mapping.readdir_entries_sync(public_parent).unwrap();
        assert_eq!(names(&parent_entries), ["pkg"]);
        assert!(parent_entries[0].is_directory);

        let entries = mapping.readdir_entries_sync(public_root).unwrap();
        assert_eq!(
            names(&entries),
            ["dup.pyi", "linked.pyi", "subdir", "real.py"]
        );
        // The real entry replaces the synthesized one in its slot.
        assert!(entries[0].is_file);
        assert!(entries[1].is_file);
        assert!(!entries[1].is_symbolic_link);
        assert!(entries[2].is_directory);
        assert!(!entries.iter().any(|entry| entry.name == "missing.pyi"));
    }

    #[test]
    fn file_system_mapping_synthesizes_a_mapped_child_with_no_public_backing_directory() {
        let public_parent = file("/public");
        let public_root = public_parent.combine_paths(&["pkg"]);
        let original_root = file("/original/pkg");
        let fs = fs_with(&[(original_root.combine_paths(&["file.py"]), "content")]);
        assert!(!fs.exists_sync(public_parent));
        let mapping = create_file_system_mapping(fs);
        mapping.map_directory(public_root, original_root, None);

        let entries = mapping.readdir_entries_sync(public_parent).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "pkg");
        assert!(entries[0].is_directory);
        assert!(!entries[0].is_file);
    }

    #[test]
    fn file_system_mapping_synthesizes_only_direct_mapped_children() {
        let public_parent = file("/public");
        let intermediate = public_parent.combine_paths(&["a"]);
        let mapped_root = intermediate.combine_paths(&["pkg"]);
        let original_root = file("/original/pkg");
        let fs = fs_with(&[(original_root.combine_paths(&["file.py"]), "content")]);
        fs.mkdirp_sync(intermediate.get_file_path());
        let mapping = create_file_system_mapping(fs);
        mapping.map_directory(mapped_root, original_root, None);

        assert_eq!(
            names(&mapping.readdir_entries_sync(public_parent).unwrap()),
            ["a"]
        );
    }

    #[test]
    fn file_system_mapping_hides_allowed_original_roots_from_their_real_parent_listing() {
        let public_root = file("/public/pkg");
        let original_parent = file("/original");
        let original_root = original_parent.combine_paths(&["pkg"]);
        let visible_sibling = original_parent.combine_paths(&["visible"]);
        let fs = fs_with(&[
            (original_root.combine_paths(&["file.py"]), "content"),
            (visible_sibling.combine_paths(&["file.py"]), "visible"),
        ]);
        let mapping = create_file_system_mapping(fs);
        mapping.map_directory(public_root, original_root, None);

        assert_eq!(
            names(&mapping.readdir_entries_sync(original_parent).unwrap()),
            ["visible"]
        );
    }

    #[test]
    #[ignore = "spies on streams and async reads, which are not ported"]
    fn file_system_mapping_preserves_read_metadata_stream_and_promise_identities() {}

    #[test]
    fn cached_structural_resolutions_evaluate_live_filters_for_every_read() {
        let public_root = file("/public/pkg");
        let original_root = file("/original/pkg");
        let public_file = public_root.combine_paths(&["file.py"]);
        let original_file = original_root.combine_paths(&["file.py"]);
        let fs = fs_with(&[(original_file, "content"), (public_file, "content")]);
        let filter_calls: FilterCalls = Rc::default();
        let visible = Rc::new(Cell::new(true));
        let mapping = create_file_system_mapping(fs);
        let is_visible = visible.clone();
        mapping.map_directory(
            public_root,
            original_root,
            recording_filter(&filter_calls, move |_| is_visible.get()),
        );

        assert_eq!(text(&mapping, public_file), "content");
        visible.set(false);
        assert_eq!(text(&mapping, public_file), "content");

        let filtered: Vec<Uri> = filter_calls.borrow().iter().map(|(u, _)| *u).collect();
        assert_eq!(filtered, [original_file, original_file]);
        let reads: Vec<String> = fs
            .calls
            .borrow()
            .iter()
            .filter(|(m, _)| *m == "readFileSync")
            .map(|(_, p)| p.clone())
            .collect();
        assert_eq!(
            reads,
            [original_file.get_file_path(), public_file.get_file_path()]
        );
    }

    #[test]
    fn file_system_mapping_keeps_internal_reads_distinct_from_chained_public_uri_translation() {
        let physical_root = file("/physical/pkg");
        let inner_public_root = file("/inner/pkg");
        let outer_public_root = file("/outer/pkg");
        let physical_file = physical_root.combine_paths(&["file.py"]);
        let inner_public_file = inner_public_root.combine_paths(&["file.py"]);
        let outer_public_file = outer_public_root.combine_paths(&["file.py"]);
        let raw = fs_with(&[(physical_file, "physical")]);
        let inner: &'static PyrightFileSystem = Box::leak(Box::new(PyrightFileSystem::new(raw)));
        inner.map_directory(inner_public_root, physical_root, None);
        let mapping = create_file_system_mapping(inner);
        mapping.map_directory(outer_public_root, inner_public_root, None);

        assert!(mapping.is_mapped_uri(inner_public_file));
        assert!(
            mapping
                .get_mapped_uri(physical_file)
                .equals(Some(inner_public_file))
        );
        assert_eq!(text(&mapping, outer_public_file), "physical");
        assert!(
            mapping
                .get_original_uri(outer_public_file)
                .equals(Some(physical_file))
        );
    }
}
