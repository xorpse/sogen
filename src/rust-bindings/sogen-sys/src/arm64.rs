#[cxx::bridge(namespace = "sogen::ffi")]
pub mod ffi {
    unsafe extern "C++" {
        include!("cpp/arm64.hpp");

        #[namespace = "sogen"]
        #[cxx_name = "backend_type"]
        type BackendType = crate::common::ffi::BackendType;
        #[namespace = "sogen"]
        #[cxx_name = "memory_permission"]
        type MemoryPermission = crate::common::ffi::MemoryPermission;
        type Machine = crate::common::ffi::Machine;

        type Arm64Machine;

        fn default_arm64_backend() -> BackendType;
        fn create_arm64_machine(backend: BackendType) -> Result<UniquePtr<Arm64Machine>>;

        fn machine(self: &Arm64Machine) -> &Machine;
        fn start(self: &Arm64Machine, count: u64) -> Result<()>;
        fn map_memory(
            self: &Arm64Machine,
            address: u64,
            size: u64,
            permissions: MemoryPermission,
        ) -> Result<()>;
        fn unmap_memory(self: &Arm64Machine, address: u64, size: u64) -> Result<bool>;
    }
}

unsafe impl Send for ffi::Arm64Machine {}
