//! Port of `packages/pyright-internal/src/analyzer/sourceFileInfo.ts`.
//!
//! Edit mode (`_preEditData`, `restore`) serves the language server and is not
//! ported: the CLI never enters edit mode, so every `mutate` writes in place.

use std::cell::{Ref, RefCell, RefMut};

use jsrt::{JsSet, P};

use crate::analyzer::source_file::{IPythonMode, SourceFile};
use crate::common::uri::Uri;

// analyzer/sourceFileInfo.ts:237
pub struct WriteableData {
    pub diagnostics_version: Option<u32>,
    pub builtins_import: Option<P<SourceFileInfo>>,
    pub chained_source_file: Option<P<SourceFileInfo>>,
    pub effective_future_imports: Option<JsSet<String>>,
    pub is_tracked: bool,
    pub is_open_by_client: bool,
    pub is_virtual: bool,
    pub imports: Vec<P<SourceFileInfo>>,
    pub imported_by: Vec<P<SourceFileInfo>>,
    pub shadows: Vec<P<SourceFileInfo>>,
    pub shadowed_by: Vec<P<SourceFileInfo>>,
}

// analyzer/sourceFileInfo.ts:227
#[derive(Default)]
pub struct OptionalArguments {
    pub is_tracked: Option<bool>,
    pub is_open_by_client: Option<bool>,
    pub is_virtual: Option<bool>,
    pub diagnostics_version: Option<u32>,
    pub builtins_import: Option<P<SourceFileInfo>>,
    pub chained_source_file: Option<P<SourceFileInfo>>,
    pub effective_future_imports: Option<JsSet<String>>,
}

// analyzer/sourceFileInfo.ts:15
/// Shared and compared by identity: `P<SourceFileInfo>`.
pub struct SourceFileInfo {
    pub source_file: P<SourceFile>,
    pub is_typeshed_file: bool,
    pub is_third_party_import: bool,
    pub is_third_party_py_typed_present: bool,
    writable_data: RefCell<WriteableData>,
}

impl SourceFileInfo {
    // analyzer/sourceFileInfo.ts:21
    pub fn new(
        source_file: P<SourceFile>,
        is_typeshed_file: bool,
        is_third_party_import: bool,
        is_third_party_py_typed_present: bool,
        args: OptionalArguments,
    ) -> P<SourceFileInfo> {
        P::new(SourceFileInfo {
            source_file,
            is_typeshed_file,
            is_third_party_import,
            is_third_party_py_typed_present,
            writable_data: RefCell::new(SourceFileInfo::create_writeable_data(args)),
        })
    }

    fn data(&self) -> Ref<'_, WriteableData> {
        self.writable_data.borrow()
    }

    // analyzer/sourceFileInfo.ts:36
    pub fn diagnostics_version(&self) -> Option<u32> {
        self.data().diagnostics_version
    }

    // analyzer/sourceFileInfo.ts:40
    pub fn builtins_import(&self) -> Option<P<SourceFileInfo>> {
        self.data().builtins_import
    }

    // analyzer/sourceFileInfo.ts:49
    pub fn chained_source_file(&self) -> Option<P<SourceFileInfo>> {
        self.data().chained_source_file
    }

    // analyzer/sourceFileInfo.ts:53
    pub fn effective_future_imports(&self) -> Option<JsSet<String>> {
        self.data().effective_future_imports.clone()
    }

    // analyzer/sourceFileInfo.ts:59
    pub fn is_tracked(&self) -> bool {
        self.data().is_tracked
    }

    // analyzer/sourceFileInfo.ts:63
    pub fn is_open_by_client(&self) -> bool {
        self.data().is_open_by_client
    }

    // analyzer/sourceFileInfo.ts:67
    pub fn is_virtual(&self) -> bool {
        self.data().is_virtual
    }

    // analyzer/sourceFileInfo.ts:70
    pub fn uri(&self) -> Uri {
        self.source_file.get_uri()
    }

    // analyzer/sourceFileInfo.ts:78
    pub fn ipython_mode(&self) -> IPythonMode {
        self.source_file.get_ipython_mode()
    }

    // analyzer/sourceFileInfo.ts:82
    pub fn is_stub_file(&self) -> bool {
        self.source_file.is_stub_file()
    }

    // analyzer/sourceFileInfo.ts:86
    pub fn is_typing_stub_file(&self) -> bool {
        self.source_file.is_typing_stub_file()
    }

    // analyzer/sourceFileInfo.ts:98
    pub fn imports(&self) -> Vec<P<SourceFileInfo>> {
        self.data().imports.clone()
    }

    // analyzer/sourceFileInfo.ts:102
    pub fn imported_by(&self) -> Vec<P<SourceFileInfo>> {
        self.data().imported_by.clone()
    }

    // analyzer/sourceFileInfo.ts:106
    pub fn are_imports_computed(&self) -> bool {
        !self.source_file.is_parse_required()
    }

    // analyzer/sourceFileInfo.ts:110
    pub fn shadows(&self) -> Vec<P<SourceFileInfo>> {
        self.data().shadows.clone()
    }

    // analyzer/sourceFileInfo.ts:114
    pub fn shadowed_by(&self) -> Vec<P<SourceFileInfo>> {
        self.data().shadowed_by.clone()
    }

    // analyzer/sourceFileInfo.ts:126
    pub fn set_diagnostics_version(&self, value: Option<u32>) {
        self.writable_data.borrow_mut().diagnostics_version = value;
    }

    // analyzer/sourceFileInfo.ts:131
    pub fn set_builtins_import(&self, value: Option<P<SourceFileInfo>>) {
        self.writable_data.borrow_mut().builtins_import = value;
    }

    // analyzer/sourceFileInfo.ts:136
    pub fn set_chained_source_file(&self, value: Option<P<SourceFileInfo>>) {
        self.writable_data.borrow_mut().chained_source_file = value;
    }

    // analyzer/sourceFileInfo.ts:141
    pub fn set_effective_future_imports(&self, value: Option<JsSet<String>>) {
        self.writable_data.borrow_mut().effective_future_imports = value;
    }

    // analyzer/sourceFileInfo.ts:146
    pub fn set_is_tracked(&self, value: bool) {
        self.writable_data.borrow_mut().is_tracked = value;
    }

    // analyzer/sourceFileInfo.ts:151
    pub fn set_is_open_by_client(&self, value: bool) {
        self.writable_data.borrow_mut().is_open_by_client = value;
    }

    // analyzer/sourceFileInfo.ts:161
    /// Never hold the borrow across a call that can reach this file again.
    pub fn mutate(&self) -> RefMut<'_, WriteableData> {
        self.writable_data.borrow_mut()
    }

    // analyzer/sourceFileInfo.ts:189
    fn create_writeable_data(args: OptionalArguments) -> WriteableData {
        WriteableData {
            is_tracked: args.is_tracked.unwrap_or(false),
            is_open_by_client: args.is_open_by_client.unwrap_or(false),
            is_virtual: args.is_virtual.unwrap_or(false),
            builtins_import: args.builtins_import,
            chained_source_file: args.chained_source_file,
            diagnostics_version: args.diagnostics_version,
            effective_future_imports: args.effective_future_imports,
            imports: Vec::new(),
            imported_by: Vec::new(),
            shadows: Vec::new(),
            shadowed_by: Vec::new(),
        }
    }
}

// analyzer/sourceFileInfoUtils.ts:14
pub fn is_user_code(file_info: Option<P<SourceFileInfo>>) -> bool {
    file_info.is_some_and(|f| f.is_tracked() && !f.is_third_party_import && !f.is_typeshed_file)
}
