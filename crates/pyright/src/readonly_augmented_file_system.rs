//! Port of `packages/pyright-internal/src/readonlyAugmentedFileSystem.ts`.
//!
//! The write methods (`mkdirSync`, `chdir`, `writeFileSync`, `rmdirSync`,
//! `unlinkSync`, `createWriteStream`, `copyFileSync`), watchers, streams and
//! async reads are not ported: the `FileSystem` trait has no write side.

use crate::common::file_system::{
    Dirent, Disposable, FileSystem, FsResult, MapDirectoryFilter, ReadOnlyFileSystem, Stats,
};
use crate::common::uri::Uri;
use crate::file_system_mapping::{FileSystemMapping, create_file_system_mapping};

// readonlyAugmentedFileSystem.ts:18
pub struct ReadOnlyAugmentedFileSystem {
    pub real_fs: &'static dyn FileSystem,
    _mapping: FileSystemMapping,
}

impl ReadOnlyAugmentedFileSystem {
    // readonlyAugmentedFileSystem.ts:21
    pub fn new(real_fs: &'static dyn FileSystem) -> ReadOnlyAugmentedFileSystem {
        ReadOnlyAugmentedFileSystem {
            real_fs,
            _mapping: create_file_system_mapping(real_fs),
        }
    }
}

impl ReadOnlyFileSystem for ReadOnlyAugmentedFileSystem {
    // readonlyAugmentedFileSystem.ts:25
    fn exists_sync(&self, uri: Uri) -> bool {
        self._mapping.exists_sync(uri)
    }

    // readonlyAugmentedFileSystem.ts:37
    fn readdir_entries_sync(&self, uri: Uri) -> FsResult<Vec<Dirent>> {
        self._mapping.readdir_entries_sync(uri)
    }

    // readonlyAugmentedFileSystem.ts:41
    fn readdir_sync(&self, uri: Uri) -> FsResult<Vec<String>> {
        Ok(self
            .readdir_entries_sync(uri)?
            .into_iter()
            .map(|p| p.name)
            .collect())
    }

    // readonlyAugmentedFileSystem.ts:45
    fn read_file_sync(&self, uri: Uri) -> FsResult<Vec<u8>> {
        self._mapping.read_file_sync(uri)
    }

    // readonlyAugmentedFileSystem.ts:45
    fn read_file_text_sync(&self, uri: Uri) -> FsResult<String> {
        self._mapping.read_file_text_sync(uri)
    }

    // readonlyAugmentedFileSystem.ts:58
    fn stat_sync(&self, uri: Uri) -> FsResult<Stats> {
        self._mapping.stat_sync(uri)
    }

    fn real_file_path(&self, uri: Uri) -> Option<String> {
        self._mapping.real_file_path(uri)
    }

    // readonlyAugmentedFileSystem.ts:70
    fn realpath_sync(&self, uri: Uri) -> Uri {
        self._mapping.realpath_sync(uri)
    }

    // readonlyAugmentedFileSystem.ts:74
    fn get_module_path(&self) -> Uri {
        self.real_fs.get_module_path()
    }

    // readonlyAugmentedFileSystem.ts:103
    fn real_case_path(&self, uri: Uri) -> Uri {
        self.real_fs.real_case_path(uri)
    }

    // readonlyAugmentedFileSystem.ts:108
    fn is_mapped_uri(&self, file_uri: Uri) -> bool {
        self._mapping.is_mapped_uri(file_uri)
    }

    // readonlyAugmentedFileSystem.ts:113
    fn get_original_uri(&self, mapped_file_uri: Uri) -> Uri {
        self._mapping.get_original_uri(mapped_file_uri)
    }

    // readonlyAugmentedFileSystem.ts:118
    fn get_mapped_uri(&self, original_file_uri: Uri) -> Uri {
        self._mapping.get_mapped_uri(original_file_uri)
    }

    // readonlyAugmentedFileSystem.ts:122
    fn is_in_zip(&self, uri: Uri) -> bool {
        self.real_fs.is_in_zip(uri)
    }
}

impl FileSystem for ReadOnlyAugmentedFileSystem {
    // readonlyAugmentedFileSystem.ts:126
    fn map_directory(
        &self,
        mapped_uri: Uri,
        original_uri: Uri,
        filter: Option<MapDirectoryFilter>,
    ) -> Disposable {
        self._mapping
            .map_directory(mapped_uri, original_uri, filter)
    }
}
