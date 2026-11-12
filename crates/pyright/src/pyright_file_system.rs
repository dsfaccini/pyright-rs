//! Port of `packages/pyright-internal/src/pyrightFileSystem.ts`.
//!
//! `PyrightFileSystem` overrides only write methods, which the `FileSystem`
//! trait does not have, so every call here forwards to
//! `ReadOnlyAugmentedFileSystem`. `IPyrightFileSystem` adds nothing to
//! `FileSystem` and is not ported.

use std::ops::Deref;

use crate::common::file_system::{
    Dirent, Disposable, FileSystem, FsResult, MapDirectoryFilter, ReadOnlyFileSystem, Stats,
};
use crate::common::uri::Uri;
use crate::readonly_augmented_file_system::ReadOnlyAugmentedFileSystem;

// pyrightFileSystem.ts:17
/// `extends ReadOnlyAugmentedFileSystem`: `Deref` reaches the base class.
pub struct PyrightFileSystem {
    base: ReadOnlyAugmentedFileSystem,
}

impl PyrightFileSystem {
    // pyrightFileSystem.ts:18
    pub fn new(real_fs: &'static dyn FileSystem) -> PyrightFileSystem {
        PyrightFileSystem {
            base: ReadOnlyAugmentedFileSystem::new(real_fs),
        }
    }
}

impl Deref for PyrightFileSystem {
    type Target = ReadOnlyAugmentedFileSystem;

    fn deref(&self) -> &ReadOnlyAugmentedFileSystem {
        &self.base
    }
}

impl ReadOnlyFileSystem for PyrightFileSystem {
    fn exists_sync(&self, uri: Uri) -> bool {
        self.base.exists_sync(uri)
    }

    fn readdir_entries_sync(&self, uri: Uri) -> FsResult<Vec<Dirent>> {
        self.base.readdir_entries_sync(uri)
    }

    fn readdir_sync(&self, uri: Uri) -> FsResult<Vec<String>> {
        self.base.readdir_sync(uri)
    }

    fn read_file_sync(&self, uri: Uri) -> FsResult<Vec<u8>> {
        self.base.read_file_sync(uri)
    }

    fn read_file_text_sync(&self, uri: Uri) -> FsResult<String> {
        self.base.read_file_text_sync(uri)
    }

    fn stat_sync(&self, uri: Uri) -> FsResult<Stats> {
        self.base.stat_sync(uri)
    }

    fn real_file_path(&self, uri: Uri) -> Option<String> {
        self.base.real_file_path(uri)
    }

    fn realpath_sync(&self, uri: Uri) -> Uri {
        self.base.realpath_sync(uri)
    }

    fn get_module_path(&self) -> Uri {
        self.base.get_module_path()
    }

    fn real_case_path(&self, uri: Uri) -> Uri {
        self.base.real_case_path(uri)
    }

    fn is_mapped_uri(&self, uri: Uri) -> bool {
        self.base.is_mapped_uri(uri)
    }

    fn get_original_uri(&self, mapped_uri: Uri) -> Uri {
        self.base.get_original_uri(mapped_uri)
    }

    fn get_mapped_uri(&self, original_uri: Uri) -> Uri {
        self.base.get_mapped_uri(original_uri)
    }

    fn is_in_zip(&self, uri: Uri) -> bool {
        self.base.is_in_zip(uri)
    }
}

impl FileSystem for PyrightFileSystem {
    fn map_directory(
        &self,
        mapped_uri: Uri,
        original_uri: Uri,
        filter: Option<MapDirectoryFilter>,
    ) -> Disposable {
        self.base.map_directory(mapped_uri, original_uri, filter)
    }
}

#[cfg(test)]
mod tests {
    //! Port of `tests/pyrightFileSystem.test.ts`.

    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::common::path_consts::{LIB, SITE_PACKAGES};
    use crate::common::path_utils::{combine_paths, get_directory_path, normalize_slashes};
    use crate::partial_stub_service::{PartialStubService, SupportPartialStubs};
    use crate::tests::harness::vfs::filesystem::{TestFileSystem, file};

    fn library_root() -> String {
        combine_paths(&normalize_slashes("/"), &[LIB, SITE_PACKAGES])
    }

    fn library_root_uri() -> Uri {
        file(&library_root())
    }

    fn lib(parts: &[&str]) -> String {
        combine_paths(&library_root(), parts)
    }

    // tests/pyrightFileSystem.test.ts:325
    fn create_file_system(files: &[(String, &str)]) -> &'static PyrightFileSystem {
        let fs = TestFileSystem::leak();

        for (path, content) in files {
            let path = normalize_slashes(path);
            let dir = get_directory_path(&path);
            fs.mkdirp_sync(&dir);

            fs.write_file_sync(&path, content);
        }

        Box::leak(Box::new(PyrightFileSystem::new(fs)))
    }

    fn partial_stub_service(fs: &'static PyrightFileSystem) -> PartialStubService {
        PartialStubService::new(fs)
    }

    #[test]
    #[ignore = "the write methods that throw 'Operation is not allowed.' are not ported"]
    fn read_only_augmented_file_system_preserves_prohibited_operations() {}

    #[test]
    fn read_only_augmented_file_system_mapping_filter_receives_its_exact_backing_file_system() {
        let original_root = file("/original");
        let mapped_root = file("/mapped");
        let original_file = original_root.combine_paths(&["file.py"]);
        let mapped_file = mapped_root.combine_paths(&["file.py"]);
        let real_fs = TestFileSystem::leak();
        real_fs.mkdirp_sync("/original");
        real_fs.write_file_sync(original_file.get_file_path(), "content");
        let fs = ReadOnlyAugmentedFileSystem::new(real_fs);
        let observed_file_system: Rc<Cell<Option<*const ()>>> = Rc::new(Cell::new(None));
        let observed = observed_file_system.clone();
        fs.map_directory(
            mapped_root,
            original_root,
            Some(Box::new(move |_uri, file_system| {
                observed.set(Some(std::ptr::from_ref(file_system).cast::<()>()));
                true
            })),
        );

        assert_eq!(fs.read_file_text_sync(mapped_file).unwrap(), "content");
        let observed = observed_file_system.get().expect("filter ran");
        assert!(std::ptr::addr_eq(
            observed,
            real_fs as *const TestFileSystem
        ));
    }

    #[test]
    #[ignore = "checks the TypeScript object surface (`'clear' in fs`); the Rust type has no such method"]
    fn read_only_augmented_file_system_exposes_no_reset_surface() {}

    #[test]
    fn virtual_file_exists() {
        let files = [
            (lib(&["myLib-stubs", "partialStub.pyi"]), "def test(): ..."),
            (
                lib(&["myLib-stubs", "subdir", "__init__.pyi"]),
                "def subdir(): ...",
            ),
            (lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            (lib(&["myLib", "partialStub.py"]), "def test(): pass"),
        ];

        let fs = create_file_system(&files);
        let ps = partial_stub_service(fs);
        ps.process_partial_stub_packages(&[library_root_uri()], &[library_root_uri()], None);

        let stub_file = library_root_uri().combine_paths(&["myLib", "partialStub.pyi"]);
        assert!(fs.exists_sync(stub_file));
        assert!(fs.is_mapped_uri(stub_file));

        let my_lib = library_root_uri().combine_paths(&["myLib"]);
        let entries = fs.readdir_entries_sync(my_lib).unwrap();
        assert_eq!(3, entries.len());

        let sub_dir_file = library_root_uri().combine_paths(&["myLib", "subdir", "__init__.pyi"]);
        assert!(fs.exists_sync(sub_dir_file));
        assert!(fs.is_mapped_uri(sub_dir_file));

        let fake_file = entries.iter().find(|e| e.name.ends_with(".pyi")).unwrap();
        assert!(fake_file.is_file);

        assert!(!fs.exists_sync(library_root_uri().combine_paths(&["myLib-stubs"])));
    }

    #[test]
    fn mapped_symlinks_reflect_their_targets() {
        let test_fs = TestFileSystem::leak();
        let stub_package = lib(&["myLib-stubs"]);
        let stub_source = combine_paths(&normalize_slashes("/"), &["wheel", "partialStub.pyi"]);
        let marker_source = combine_paths(&normalize_slashes("/"), &["wheel", "py.typed"]);
        let missing_stub = combine_paths(&stub_package, &["missing.pyi"]);

        test_fs.mkdirp_sync(&lib(&["myLib"]));
        test_fs.write_file_sync(&lib(&["myLib", "partialStub.py"]), "def test(): pass");
        test_fs.mkdirp_sync(&stub_package);
        test_fs.mkdirp_sync(&get_directory_path(&stub_source));
        test_fs.write_file_sync(&stub_source, "def test(): ...");
        test_fs.write_file_sync(&marker_source, "partial\n");
        test_fs.symlink_sync(
            &stub_source,
            &combine_paths(&stub_package, &["partialStub.pyi"]),
        );
        test_fs.symlink_sync(&marker_source, &combine_paths(&stub_package, &["py.typed"]));
        test_fs.symlink_sync(
            &combine_paths(&normalize_slashes("/"), &["wheel", "missing.pyi"]),
            &missing_stub,
        );

        let fs: &'static PyrightFileSystem = Box::leak(Box::new(PyrightFileSystem::new(test_fs)));
        let ps = partial_stub_service(fs);
        ps.process_partial_stub_packages(&[library_root_uri()], &[library_root_uri()], None);

        let entries = fs
            .readdir_entries_sync(library_root_uri().combine_paths(&["myLib"]))
            .unwrap();
        assert_eq!(2, entries.len());
        let stub_file = entries
            .iter()
            .find(|entry| entry.name == "partialStub.pyi")
            .expect("Expected partialStub.pyi");
        assert!(stub_file.is_file);
        assert!(!entries.iter().any(|entry| entry.name == "missing.pyi"));
    }

    #[test]
    fn virtual_file_coexists_with_real() {
        let files = [
            (lib(&["myLib-stubs", "partialStub.pyi"]), "def test(): ..."),
            (
                lib(&["myLib-stubs", "subdir", "__init__.pyi"]),
                "def subdir(): ...",
            ),
            (lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            (lib(&["myLib", "partialStub.py"]), "def test(): pass"),
            (lib(&["myLib", "subdir", "__init__.py"]), "def test(): pass"),
        ];

        let fs = create_file_system(&files);
        let ps = partial_stub_service(fs);
        ps.process_partial_stub_packages(&[library_root_uri()], &[library_root_uri()], None);

        let stub_file = library_root_uri().combine_paths(&["myLib", "partialStub.pyi"]);
        assert!(fs.exists_sync(stub_file));
        assert!(fs.is_mapped_uri(stub_file));

        let my_lib = library_root_uri().combine_paths(&["myLib"]);
        let entries = fs.readdir_entries_sync(my_lib).unwrap();
        assert_eq!(3, entries.len());

        let sub_dir_file = library_root_uri().combine_paths(&["myLib", "subdir", "__init__.pyi"]);
        assert!(fs.exists_sync(sub_dir_file));
        assert!(fs.is_mapped_uri(sub_dir_file));

        let sub_dir_pyi_file =
            library_root_uri().combine_paths(&["myLib", "subdir", "__init__.pyi"]);
        assert!(fs.exists_sync(sub_dir_pyi_file));

        let fake_file = entries.iter().find(|e| e.name.ends_with(".pyi")).unwrap();
        assert!(fake_file.is_file);

        assert!(!fs.exists_sync(library_root_uri().combine_paths(&["myLib-stubs"])));
    }

    #[test]
    fn virtual_file_not_exist() {
        let files = [
            (lib(&["myLib-stubs", "partialStub.pyi"]), "def test(): ..."),
            (lib(&["myLib", "otherType.py"]), "def test(): pass"),
        ];

        let fs = create_file_system(&files);
        let ps = partial_stub_service(fs);
        ps.process_partial_stub_packages(&[library_root_uri()], &[library_root_uri()], None);

        assert!(!fs.exists_sync(library_root_uri().combine_paths(&["myLib", "partialStub.pyi"])));

        let my_lib = library_root_uri().combine_paths(&["myLib"]);
        let entries = fs.readdir_entries_sync(my_lib).unwrap();
        assert_eq!(1, entries.len());

        assert_eq!(
            0,
            entries.iter().filter(|e| e.name.ends_with(".pyi")).count()
        );

        assert!(fs.exists_sync(library_root_uri().combine_paths(&["myLib-stubs"])));
    }

    #[test]
    fn existing_stub_file() {
        let files = [
            (lib(&["myLib-stubs", "partialStub.pyi"]), "def test(): ..."),
            (lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            (lib(&["myLib", "partialStub.py"]), "def test(): pass"),
            (lib(&["myLib", "partialStub.pyi"]), "def test(): pass"),
        ];

        let fs = create_file_system(&files);
        let ps = partial_stub_service(fs);
        ps.process_partial_stub_packages(&[library_root_uri()], &[library_root_uri()], None);

        let stub_file = library_root_uri().combine_paths(&["myLib", "partialStub.pyi"]);
        assert!(fs.exists_sync(stub_file));

        let my_lib = library_root_uri().combine_paths(&["myLib"]);
        let entries = fs.readdir_entries_sync(my_lib).unwrap();
        assert_eq!(2, entries.len());

        assert_eq!(
            "def test(): ...",
            fs.read_file_text_sync(stub_file).unwrap()
        );

        assert!(!fs.exists_sync(library_root_uri().combine_paths(&["myLib-stubs"])));
    }

    #[test]
    fn multiple_package_installed() {
        let extra_root = combine_paths(&normalize_slashes("/"), &[LIB, "extra"]);
        let extra_root_uri = file(&extra_root);
        let files = [
            (lib(&["myLib-stubs", "partialStub.pyi"]), "def test(): ..."),
            (lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            (lib(&["myLib", "partialStub.py"]), "def test(): pass"),
            (
                combine_paths(&extra_root, &["myLib", "partialStub.py"]),
                "def test(): pass",
            ),
        ];

        let fs = create_file_system(&files);
        let ps = partial_stub_service(fs);
        ps.process_partial_stub_packages(
            &[library_root_uri(), extra_root_uri],
            &[library_root_uri(), extra_root_uri],
            None,
        );

        assert!(ps.is_path_scanned(library_root_uri()));
        assert!(ps.is_path_scanned(extra_root_uri));

        assert!(fs.exists_sync(library_root_uri().combine_paths(&["myLib", "partialStub.pyi"])));
        assert!(fs.exists_sync(extra_root_uri.combine_paths(&["myLib", "partialStub.pyi"])));

        assert_eq!(
            2,
            fs.readdir_entries_sync(library_root_uri().combine_paths(&["myLib"]))
                .unwrap()
                .len()
        );
        assert_eq!(
            2,
            fs.readdir_entries_sync(extra_root_uri.combine_paths(&["myLib"]))
                .unwrap()
                .len()
        );
    }

    #[test]
    fn bundled_partial_stubs() {
        let bundled_path = combine_paths(&normalize_slashes("/"), &["bundled"]);
        let bundled_path_uri = file(&bundled_path);

        let files = [
            (
                combine_paths(&bundled_path, &["myLib-stubs", "partialStub.pyi"]),
                "def test(): ...",
            ),
            (
                combine_paths(&bundled_path, &["myLib-stubs", "py.typed"]),
                "partial\n",
            ),
            (lib(&["myLib", "partialStub.py"]), "def test(): pass"),
            (lib(&["myLib", "py.typed"]), ""),
        ];

        let fs = create_file_system(&files);
        let ps = partial_stub_service(fs);
        ps.process_partial_stub_packages(
            &[bundled_path_uri],
            &[library_root_uri()],
            Some(bundled_path_uri),
        );

        let stub_file = library_root_uri().combine_paths(&["myLib", "partialStub.pyi"]);
        assert!(!fs.exists_sync(stub_file));

        let my_lib = library_root_uri().combine_paths(&["myLib"]);
        let entries = fs.readdir_entries_sync(my_lib).unwrap();
        assert_eq!(2, entries.len());
    }

    #[test]
    fn clear_partial_stubs_unmaps_and_forgets_scanned_paths() {
        let files = [
            (lib(&["myLib-stubs", "partialStub.pyi"]), "def test(): ..."),
            (lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            (lib(&["myLib", "partialStub.py"]), "def test(): pass"),
        ];

        let fs = create_file_system(&files);
        let ps = partial_stub_service(fs);
        ps.process_partial_stub_packages(&[library_root_uri()], &[library_root_uri()], None);
        let stub_file = library_root_uri().combine_paths(&["myLib", "partialStub.pyi"]);
        assert!(fs.exists_sync(stub_file));

        ps.clear_partial_stubs();
        assert!(!ps.is_path_scanned(library_root_uri()));
        assert!(!fs.exists_sync(stub_file));
        assert!(fs.exists_sync(library_root_uri().combine_paths(&["myLib-stubs"])));
    }
}
