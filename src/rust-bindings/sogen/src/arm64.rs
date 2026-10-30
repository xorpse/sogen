//! A bare ARM64 CPU with guest memory, without an operating system personality.

use std::cell::Cell;
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use cxx::UniquePtr;
use sogen_sys::arm64::ffi;
use sogen_sys::common::ffi::Machine;

use crate::error::Error;
use crate::machine::{
    Backend, ContextPtr, Guest, MemoryPermission, Owned, Sealed, StopHandle, StopTarget,
};

/// The backend used when none is requested: hardware virtualisation where usable, Unicorn otherwise.
pub fn default_backend() -> Backend {
    ffi::default_arm64_backend()
}

pub struct Emulator {
    context: Owned<Context>,
}

impl Emulator {
    /// Without a backend, [`default_backend`] applies.
    pub fn new(backend: Option<Backend>) -> Result<Self, Error> {
        let native = ffi::create_arm64_machine(backend.unwrap_or_else(default_backend))?;
        Ok(Self {
            context: Owned::new(Context {
                native,
                stop: OnceLock::new(),
                not_sync: PhantomData,
            }),
        })
    }

    pub fn run(&mut self) -> Result<(), Error> {
        self.run_instructions(0)
    }

    /// Runs at most `count` instructions; zero means no limit.
    pub fn run_instructions(&mut self, count: u64) -> Result<(), Error> {
        Ok(self.context.native.start(count)?)
    }
}

impl Deref for Emulator {
    type Target = Context;

    fn deref(&self) -> &Context {
        &self.context
    }
}

pub struct Context {
    native: UniquePtr<ffi::Arm64Machine>,
    stop: OnceLock<Arc<StopTarget>>,
    not_sync: PhantomData<Cell<()>>,
}

impl Context {
    pub fn map_memory(
        &self,
        address: u64,
        size: u64,
        permissions: MemoryPermission,
    ) -> Result<(), Error> {
        Ok(self.native.map_memory(address, size, permissions.into())?)
    }

    /// Returns false if the range was not mapped.
    pub fn unmap_memory(&self, address: u64, size: u64) -> Result<bool, Error> {
        Ok(self.native.unmap_memory(address, size)?)
    }

    pub fn stop(&self) {
        self.native.machine().stop();
    }

    /// A handle that stops the guest from another thread, where the backend supports that.
    pub fn stop_handle(&self) -> Result<StopHandle, Error> {
        if !self.native.machine().is_stop_thread_safe() {
            return Err(Error::StopFromAnotherThread);
        }

        let context = ContextPtr::new(self);
        let target = self
            .stop
            .get_or_init(|| StopTarget::new(move || context.get().native.machine().stop()));
        Ok(target.handle())
    }
}

impl Sealed for Context {
    fn native_machine(&self) -> &Machine {
        self.native.machine()
    }
}

impl Guest for Context {}

impl Drop for Context {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.get() {
            stop.detach();
        }
    }
}
