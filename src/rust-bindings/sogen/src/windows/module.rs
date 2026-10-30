use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sogen_sys::windows::ffi;

use crate::machine::MemoryPermission;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedSymbol {
    name: String,
    ordinal: u64,
    rva: u64,
    address: u64,
}

impl ExportedSymbol {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ordinal(&self) -> u64 {
        self.ordinal
    }

    pub fn rva(&self) -> u64 {
        self.rva
    }

    pub fn address(&self) -> u64 {
        self.address
    }
}

/// An import address table slot of a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    module: String,
    name: String,
    slot: u64,
}

impl Import {
    /// The imported module as the import directory names it, such as `KERNEL32.dll`.
    pub fn module(&self) -> &str {
        &self.module
    }

    /// The imported name, or `#` and the ordinal for an import by ordinal.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The address of the slot the loader writes the resolved address to.
    pub fn slot(&self) -> u64 {
        self.slot
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    name: String,
    start: u64,
    length: u64,
    permissions: MemoryPermission,
    first_execution: Option<u64>,
}

impl Section {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn permissions(&self) -> MemoryPermission {
        self.permissions
    }

    /// The address the guest first executed in the section, once it has.
    pub fn first_execution(&self) -> Option<u64> {
        self.first_execution
    }

    pub fn contains(&self, address: u64) -> bool {
        address.wrapping_sub(self.start) < self.length
    }
}

/// A snapshot of an image mapped into the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    name: String,
    path: PathBuf,
    module_path: String,
    image_base: u64,
    image_base_file: u64,
    size_of_image: u64,
    entry_point: u64,
    is_static: bool,
    machine: u16,
    characteristics: u16,
    dll_characteristics: u16,
    size_of_stack_reserve: u64,
    size_of_stack_commit: u64,
    size_of_heap_reserve: u64,
    size_of_heap_commit: u64,
    exports: Vec<ExportedSymbol>,
    address_names: BTreeMap<u64, String>,
    sections: Vec<Section>,
    imports: Vec<Import>,
    imported_modules: Vec<String>,
}

impl Module {
    pub(super) fn from_native(module: ffi::ModuleInfo) -> Self {
        let mut imports = module
            .imports
            .into_iter()
            .map(|import| Import {
                module: import.module,
                name: import.name,
                slot: import.slot,
            })
            .collect::<Vec<_>>();
        imports.sort_unstable_by_key(Import::slot);

        Self {
            name: module.name,
            path: PathBuf::from(module.path),
            module_path: module.module_path,
            image_base: module.image_base,
            image_base_file: module.image_base_file,
            size_of_image: module.size_of_image,
            entry_point: module.entry_point,
            is_static: module.is_static,
            machine: module.machine,
            characteristics: module.characteristics,
            dll_characteristics: module.dll_characteristics,
            size_of_stack_reserve: module.size_of_stack_reserve,
            size_of_stack_commit: module.size_of_stack_commit,
            size_of_heap_reserve: module.size_of_heap_reserve,
            size_of_heap_commit: module.size_of_heap_commit,
            exports: module
                .exports
                .into_iter()
                .map(|symbol| ExportedSymbol {
                    name: symbol.name,
                    ordinal: symbol.ordinal,
                    rva: symbol.rva,
                    address: symbol.address,
                })
                .collect(),
            address_names: module
                .address_names
                .into_iter()
                .map(|entry| (entry.address, entry.name))
                .collect(),
            sections: module
                .sections
                .into_iter()
                .map(|section| Section {
                    name: section.name,
                    start: section.start,
                    length: section.length,
                    permissions: MemoryPermission::from(section.permissions),
                    first_execution: section
                        .has_first_execution
                        .then_some(section.first_execution),
                })
                .collect(),
            imports,
            imported_modules: module.imported_modules,
        }
    }

    /// The file name, such as `kernel32.dll`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The file on the host the image was mapped from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The path the guest sees, such as `C:\Windows\System32\kernel32.dll`.
    pub fn module_path(&self) -> &str {
        &self.module_path
    }

    pub fn image_base(&self) -> u64 {
        self.image_base
    }

    /// The preferred image base from the PE header.
    pub fn image_base_file(&self) -> u64 {
        self.image_base_file
    }

    pub fn size_of_image(&self) -> u64 {
        self.size_of_image
    }

    pub fn entry_point(&self) -> u64 {
        self.entry_point
    }

    pub fn is_static(&self) -> bool {
        self.is_static
    }

    /// The COFF machine type, such as `0x8664` for AMD64.
    pub fn machine(&self) -> u16 {
        self.machine
    }

    pub fn characteristics(&self) -> u16 {
        self.characteristics
    }

    pub fn dll_characteristics(&self) -> u16 {
        self.dll_characteristics
    }

    pub fn size_of_stack_reserve(&self) -> u64 {
        self.size_of_stack_reserve
    }

    pub fn size_of_stack_commit(&self) -> u64 {
        self.size_of_stack_commit
    }

    pub fn size_of_heap_reserve(&self) -> u64 {
        self.size_of_heap_reserve
    }

    pub fn size_of_heap_commit(&self) -> u64 {
        self.size_of_heap_commit
    }

    pub fn exports(&self) -> &[ExportedSymbol] {
        &self.exports
    }

    /// One export name per exported address.
    pub fn address_names(&self) -> &BTreeMap<u64, String> {
        &self.address_names
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Ordered by slot address.
    pub fn imports(&self) -> &[Import] {
        &self.imports
    }

    /// The modules named by the import directory, in its order.
    pub fn imported_modules(&self) -> &[String] {
        &self.imported_modules
    }

    pub fn contains(&self, address: u64) -> bool {
        address.wrapping_sub(self.image_base) < self.size_of_image
    }
}

/// How [`Context::map_module`](super::Context::map_module) and its siblings map an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleOptions {
    is_static: bool,
    allow_duplicate: bool,
    relocation_base: Option<u64>,
}

impl ModuleOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// A static module stays mapped when it is unmapped.
    pub fn is_static(&self) -> bool {
        self.is_static
    }

    pub fn set_is_static(&mut self, is_static: bool) {
        self.is_static = is_static;
    }

    pub fn with_is_static(mut self, is_static: bool) -> Self {
        self.set_is_static(is_static);
        self
    }

    /// Maps the image again even when the same file or base is already mapped, which otherwise returns that module.
    pub fn allow_duplicate(&self) -> bool {
        self.allow_duplicate
    }

    pub fn set_allow_duplicate(&mut self, allow_duplicate: bool) {
        self.allow_duplicate = allow_duplicate;
    }

    pub fn with_allow_duplicate(mut self, allow_duplicate: bool) -> Self {
        self.set_allow_duplicate(allow_duplicate);
        self
    }

    /// The base to relocate a mapped file to instead of its preferred base. Images already in guest memory stay where
    /// they are.
    pub fn relocation_base(&self) -> Option<u64> {
        self.relocation_base
    }

    pub fn set_relocation_base(&mut self, relocation_base: u64) {
        self.relocation_base = Some(relocation_base);
    }

    pub fn with_relocation_base(mut self, relocation_base: u64) -> Self {
        self.set_relocation_base(relocation_base);
        self
    }

    pub(super) fn to_native(self) -> ffi::MappingOptions {
        ffi::MappingOptions {
            is_static: self.is_static,
            allow_duplicate: self.allow_duplicate,
            relocation_base: self.relocation_base.unwrap_or(0),
        }
    }
}
