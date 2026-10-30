use cxx::UniquePtr;
use sogen_sys::windows::ffi;

use super::Application;
use crate::error::Error;

/// The connection of a host process started for a guest child process, see
/// [`ChildProcesses`](super::ChildProcesses).
pub struct ManagedProcess {
    native: UniquePtr<ffi::ManagedProcess>,
}

impl ManagedProcess {
    /// `None` unless `arguments` (normally `std::env::args().skip(1)`) end with the managed-process arguments;
    /// connects to the parent otherwise. The arguments stay in place, so the program's own argument parsing must
    /// tolerate or strip the trailing `--sogen-managed-process <port> <token>`.
    pub fn from_arguments(
        arguments: impl IntoIterator<Item = String>,
    ) -> Result<Option<Self>, Error> {
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        let native = ffi::connect_managed_process(&arguments)?;
        Ok((!native.is_null()).then_some(Self { native }))
    }

    /// The guest application to run, with the parent's command line, ids, working directory and environment.
    pub fn application(&self) -> Application {
        Application::from_native(self.native.application())
    }

    pub(super) fn into_native(self) -> UniquePtr<ffi::ManagedProcess> {
        self.native
    }
}
