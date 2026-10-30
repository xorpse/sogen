use std::time::Duration;

use sogen_sys::windows::ffi;

pub use sogen_sys::windows::ffi::{EventType, HandleType};

/// The state of a kernel object behind a handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectInfo {
    Event {
        name: String,
        kind: EventType,
        signalled: bool,
    },
    File {
        path: String,
        is_directory: bool,
    },
    Job {
        name: String,
        limit_flags: u32,
    },
    Mutant {
        name: String,
        owner_thread_id: Option<u32>,
        lock_count: u32,
        abandoned: bool,
    },
    /// An object whose state the emulator does not report.
    Other,
    Port {
        name: String,
    },
    Process {
        id: u32,
    },
    RegistryKey {
        path: String,
    },
    Section {
        name: String,
        file_name: String,
        maximum_size: u64,
        page_protection: u32,
        allocation_attributes: u32,
    },
    Semaphore {
        name: String,
        count: u32,
        maximum: u32,
    },
    Thread {
        id: u32,
    },
    Timer {
        name: String,
        /// The time left on the emulator clock until the timer is due, if it is set.
        due_in: Option<Duration>,
        period: Duration,
        signalled: bool,
    },
}

/// A snapshot of a guest handle and its object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleInfo {
    value: u64,
    kind: HandleType,
    object: ObjectInfo,
}

impl HandleInfo {
    pub(super) fn from_native(record: ffi::HandleRecord) -> Self {
        let object = match record.kind {
            HandleType::Event => ObjectInfo::Event {
                name: record.name,
                kind: record.event_type,
                signalled: record.signalled,
            },
            HandleType::File => ObjectInfo::File {
                path: record.path,
                is_directory: record.is_directory,
            },
            HandleType::Job => ObjectInfo::Job {
                name: record.name,
                limit_flags: record.limit_flags,
            },
            HandleType::Mutant => ObjectInfo::Mutant {
                name: record.name,
                owner_thread_id: (record.id != 0).then_some(record.id),
                lock_count: record.current_count,
                abandoned: record.abandoned,
            },
            HandleType::Port => ObjectInfo::Port { name: record.name },
            HandleType::Process => ObjectInfo::Process { id: record.id },
            HandleType::Registry => ObjectInfo::RegistryKey { path: record.path },
            HandleType::Section => ObjectInfo::Section {
                name: record.name,
                file_name: record.path,
                maximum_size: record.maximum_size,
                page_protection: record.page_protection,
                allocation_attributes: record.allocation_attributes,
            },
            HandleType::Semaphore => ObjectInfo::Semaphore {
                name: record.name,
                count: record.current_count,
                maximum: record.maximum_count,
            },
            HandleType::Thread => ObjectInfo::Thread { id: record.id },
            HandleType::Timer => ObjectInfo::Timer {
                name: record.name,
                due_in: record
                    .has_due_time
                    .then(|| Duration::from_nanos(record.due_in_nanoseconds)),
                period: Duration::from_nanos(record.period_nanoseconds),
                signalled: record.signalled,
            },
            _ => ObjectInfo::Other,
        };

        Self {
            value: record.value,
            kind: record.kind,
            object,
        }
    }

    /// The handle value the guest uses.
    pub fn value(&self) -> u64 {
        self.value
    }

    pub fn kind(&self) -> HandleType {
        self.kind
    }

    pub fn object(&self) -> &ObjectInfo {
        &self.object
    }
}
