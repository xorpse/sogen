use std::alloc::{self, Layout};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use sogen_sys::windows::{MmioDeviceBridge, ffi};

use crate::machine::{MemoryPermission, MemoryRegionKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allocation {
    size: u64,
    permissions: MemoryPermission,
    reserve_only: bool,
    start: Option<u64>,
    highest_address: Option<u64>,
    kind: MemoryRegionKind,
}

impl Allocation {
    /// Committed private memory anywhere above the default allocation address.
    pub fn new(size: u64, permissions: MemoryPermission) -> Self {
        Self {
            size,
            permissions,
            reserve_only: false,
            start: None,
            highest_address: None,
            kind: MemoryRegionKind::PrivateAllocation,
        }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn permissions(&self) -> MemoryPermission {
        self.permissions
    }

    pub fn reserve_only(&self) -> bool {
        self.reserve_only
    }

    pub fn set_reserve_only(&mut self, reserve_only: bool) {
        self.reserve_only = reserve_only;
    }

    pub fn with_reserve_only(mut self, reserve_only: bool) -> Self {
        self.set_reserve_only(reserve_only);
        self
    }

    /// The lowest address to search from; without one, the default allocation address.
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

    /// The highest address the allocation may cover; without one, the top of the allocatable range.
    pub fn highest_address(&self) -> Option<u64> {
        self.highest_address
    }

    pub fn set_highest_address(&mut self, highest_address: u64) {
        self.highest_address = Some(highest_address);
    }

    pub fn with_highest_address(mut self, highest_address: u64) -> Self {
        self.set_highest_address(highest_address);
        self
    }

    pub fn kind(&self) -> MemoryRegionKind {
        self.kind
    }

    pub fn set_kind(&mut self, kind: MemoryRegionKind) {
        self.kind = kind;
    }

    pub fn with_kind(mut self, kind: MemoryRegionKind) -> Self {
        self.set_kind(kind);
        self
    }
}

/// The region of guest memory containing an address, as `NtQueryVirtualMemory` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryRegion {
    start: u64,
    length: u64,
    permissions: MemoryPermission,
    allocation_base: u64,
    allocation_length: u64,
    is_reserved: bool,
    is_committed: bool,
    initial_permissions: MemoryPermission,
    kind: MemoryRegionKind,
}

impl MemoryRegion {
    pub(super) fn from_native(region: ffi::RegionInfo) -> Self {
        Self {
            start: region.start,
            length: region.length,
            permissions: MemoryPermission::from(region.permissions),
            allocation_base: region.allocation_base,
            allocation_length: region.allocation_length,
            is_reserved: region.is_reserved,
            is_committed: region.is_committed,
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

    /// Includes [`MemoryPermission::GUARD`] for guard pages.
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
        self.is_reserved
    }

    pub fn is_committed(&self) -> bool {
        self.is_committed
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
    reserved_memory: u64,
    committed_memory: u64,
}

impl MemoryStats {
    pub(super) fn from_native(stats: ffi::MemoryStats) -> Self {
        Self {
            reserved_memory: stats.reserved_memory,
            committed_memory: stats.committed_memory,
        }
    }

    pub fn reserved_memory(&self) -> u64 {
        self.reserved_memory
    }

    pub fn committed_memory(&self) -> u64 {
        self.committed_memory
    }
}

/// Device memory: guest reads and writes of a mapped range call the device with the offset into the range. Backends
/// may split or merge accesses and read whole pages ahead of the guest, and some ignore writes (FEX). Callbacks run on
/// the host thread driving the accessing virtual CPU without the kernel lock, possibly concurrently and possibly inside
/// a [`Context`](super::Context) call, so they receive no context.
pub trait MmioDevice: Send + Sync + 'static {
    fn read(&self, offset: u64, data: &mut [u8]);
    fn write(&self, offset: u64, data: &[u8]);
}

pub(super) struct MmioDeviceAdapter<D>(pub(super) D);

impl<D: MmioDevice> MmioDeviceBridge for MmioDeviceAdapter<D> {
    fn read(&self, offset: u64, data: &mut [u8]) {
        self.0.read(offset, data);
    }

    fn write(&self, offset: u64, data: &[u8]) {
        self.0.write(offset, data);
    }
}

const HOST_PAGE_SIZE: usize = 0x4000;

/// Zeroed host memory, aligned to 16 KiB pages, that guest memory can alias without copying. Clones share the memory.
#[derive(Clone)]
pub struct HostMemory(Arc<HostBuffer>);

impl HostMemory {
    /// `size` is rounded up to whole 16 KiB pages, at least one.
    pub fn new(size: u64) -> Self {
        let size = usize::try_from(size)
            .ok()
            .and_then(|size| size.max(1).checked_next_multiple_of(HOST_PAGE_SIZE))
            .expect("host memory size exceeds the address space");
        let layout = Layout::from_size_align(size, HOST_PAGE_SIZE)
            .expect("host memory size exceeds the address space");
        let Some(bytes) = NonNull::new(unsafe { alloc::alloc_zeroed(layout) }) else {
            alloc::handle_alloc_error(layout);
        };
        Self(Arc::new(HostBuffer { bytes, layout }))
    }

    pub fn size(&self) -> u64 {
        self.0.layout.size() as u64
    }

    /// Panics if the range exceeds the memory, like slice indexing.
    pub fn read(&self, offset: u64, data: &mut [u8]) {
        let bytes = self.0.range(offset, data.len());
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = unsafe { AtomicU8::from_ptr(bytes.add(index)) }.load(Ordering::Relaxed);
        }
    }

    /// Panics if the range exceeds the memory, like slice indexing.
    pub fn write(&self, offset: u64, data: &[u8]) {
        let bytes = self.0.range(offset, data.len());
        for (index, byte) in data.iter().enumerate() {
            unsafe { AtomicU8::from_ptr(bytes.add(index)) }.store(*byte, Ordering::Relaxed);
        }
    }

    pub(super) fn as_mut_ptr(&self) -> *mut u8 {
        self.0.bytes.as_ptr()
    }
}

/// Guest stores race host accesses, so the bytes are only touched through raw pointers and atomic accesses, never
/// through Rust references.
struct HostBuffer {
    bytes: NonNull<u8>,
    layout: Layout,
}

unsafe impl Send for HostBuffer {}

unsafe impl Sync for HostBuffer {}

impl HostBuffer {
    fn range(&self, offset: u64, length: usize) -> *mut u8 {
        let end = usize::try_from(offset)
            .ok()
            .and_then(|offset| offset.checked_add(length).map(|end| (offset, end)));
        match end {
            Some((offset, end)) if end <= self.layout.size() => unsafe {
                self.bytes.as_ptr().add(offset)
            },
            _ => panic!(
                "range at offset {offset} of length {length} exceeds host memory of size {}",
                self.layout.size()
            ),
        }
    }
}

impl Drop for HostBuffer {
    fn drop(&mut self) {
        unsafe { alloc::dealloc(self.bytes.as_ptr(), self.layout) };
    }
}
