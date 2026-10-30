use sogen_sys::linux::ffi;

use crate::machine::{MemoryPermission, MemoryRegionKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allocation {
    size: u64,
    permissions: MemoryPermission,
    start: Option<u64>,
}

impl Allocation {
    pub fn new(size: u64, permissions: MemoryPermission) -> Self {
        Self {
            size,
            permissions,
            start: None,
        }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn permissions(&self) -> MemoryPermission {
        self.permissions
    }

    /// The lowest address to search from; without one the search starts at the mmap base.
    pub fn start(&self) -> Option<u64> {
        self.start
    }

    pub fn set_start(&mut self, start: u64) {
        self.start = Some(start);
    }

    pub fn with_start(mut self, start: u64) -> Self {
        self.set_start(start);
        self
    }
}

/// A mapping of guest memory. Linux has no reserve/commit split, so every mapping is committed private memory whose
/// allocation is the mapping itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryRegion {
    start: u64,
    length: u64,
    permissions: MemoryPermission,
    allocation_base: u64,
    allocation_length: u64,
    reserved: bool,
    committed: bool,
    initial_permissions: MemoryPermission,
    kind: MemoryRegionKind,
}

impl MemoryRegion {
    pub(super) fn from_native(region: ffi::LinuxMemoryRegion) -> Self {
        Self {
            start: region.start,
            length: region.length,
            permissions: MemoryPermission::from(region.permissions),
            allocation_base: region.allocation_base,
            allocation_length: region.allocation_length,
            reserved: region.reserved,
            committed: region.committed,
            initial_permissions: MemoryPermission::from(region.initial_permissions),
            kind: region.kind,
        }
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

    pub fn allocation_base(&self) -> u64 {
        self.allocation_base
    }

    pub fn allocation_length(&self) -> u64 {
        self.allocation_length
    }

    pub fn is_reserved(&self) -> bool {
        self.reserved
    }

    pub fn is_committed(&self) -> bool {
        self.committed
    }

    pub fn initial_permissions(&self) -> MemoryPermission {
        self.initial_permissions
    }

    pub fn kind(&self) -> MemoryRegionKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryStats {
    region_count: u64,
    mapped_bytes: u64,
    executable_bytes: u64,
}

impl MemoryStats {
    pub(super) fn from_native(stats: ffi::LinuxMemoryStats) -> Self {
        Self {
            region_count: stats.region_count,
            mapped_bytes: stats.mapped_bytes,
            executable_bytes: stats.executable_bytes,
        }
    }

    pub fn region_count(&self) -> u64 {
        self.region_count
    }

    pub fn mapped_bytes(&self) -> u64 {
        self.mapped_bytes
    }

    pub fn executable_bytes(&self) -> u64 {
        self.executable_bytes
    }
}
