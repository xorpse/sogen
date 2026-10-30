# sogen

Rust bindings to [Sogen](https://github.com/momo5502/sogen), a Windows and Linux user-space emulator.

- `sogen::windows`: Windows processes (x86-64, WoW64 and ARM64 guests), API hooks, event callbacks, snapshots.
- `sogen::linux`: Linux x86-64 processes, symbol hooks, event callbacks, a debugger facade.
- `sogen::arm64`: a bare ARM64 CPU with guest memory.

Every personality implements [`Guest`] for memory, registers and low-level hooks.

## Building

The crate builds Sogen from its source tree with CMake, so it is used as a git dependency:

```toml
[dependencies]
sogen = { git = "https://github.com/momo5502/sogen" }
```

It needs CMake, a C++20 compiler and the repository's submodules (cargo fetches them for git dependencies). Set
`CMAKE_GENERATOR=Ninja` for faster builds.

Features:

- `static` (default): links Sogen statically.
- `shared`: builds Sogen's backends as shared libraries and links them dynamically.
- `sdl`: gives Windows emulators SDL windows and audio. Without it, which is the default, they use null UI and audio
  backends, and SDL is neither built nor linked.

Some backends are always shared libraries (FEXCore on ARM64 hosts), as are all of them with `shared`. Add `sogen-build`
as a build dependency and call it from your build script to put their directories on the rpath of your binaries, tests
and examples. It reads them from `sogen`, so the package needs a direct dependency on `sogen`:

```toml
[build-dependencies]
sogen-build = { git = "https://github.com/momo5502/sogen" }
```

```rust,ignore
// build.rs
fn main() {
    sogen_build::configure_linkage();
}
```

Windows has no rpath; there, binaries run outside of Cargo need `sogen_build::runtime_dirs()` on `PATH`.

On macOS, the Hypervisor.framework backend is only used by binaries signed with the hypervisor entitlement
(`codesign --force --sign - --entitlements src/backends/hvf-arm64-emulator/hypervisor.entitlements <binary>`); others
fall back to Unicorn.

The icicle backend reads Ghidra processor specifications from `$GHIDRA_SRC/Ghidra/Processors`, or `./Ghidra/Processors`
without `GHIDRA_SRC`. Point `GHIDRA_SRC` at `src/backends/icicle-emulator/icicle-bridge/data` to use it.

## Example

Windows guests need an emulation root, for example the [ready-made one](https://sogen.dev/root.zip).

```rust,no_run
use sogen::windows::{ApiSignature, Application, CallingConvention, Emulator, Settings};
use sogen::CallContinuation;

fn main() -> Result<(), sogen::Error> {
    let settings = Settings::new().with_emulation_root("./root");
    let mut emulator = Emulator::application(&settings, &Application::new(r"C:\test-sample.exe"))?;

    emulator.on_stdout(|_, data| print!("{}", String::from_utf8_lossy(data)));
    emulator.hook_api("kernel32!Sleep", ApiSignature::new(CallingConvention::Stdcall, 1), |_, call, arguments| {
        println!("{} slept for {} ms", call.name(), arguments[0]);
        CallContinuation::RunOriginal
    })?;

    emulator.run()?;
    println!("exit status: {:?}", emulator.process().exit_status());
    Ok(())
}
```

`examples/unpack.rs` is an unpacker for 64-bit executables. It runs the sample until the stub hands over to the
original program, either by jumping to code it wrote into the executable's image (in-place packers such as UPX) or by
entering a PE image it mapped into private memory (reflective loaders such as Amber), and dumps that image as a PE
file starting at the original entry point:

```text
cargo run --example unpack -- ./root 'C:\packed.exe' unpacked.exe
```

## Callbacks and threads

Callbacks receive the emulator's `Context`, which gives access to everything except running the guest. They take
`Fn` closures because a callback can be re-entered, for example when it causes the event it observes. A panic in a
callback aborts the process.

`Emulator` is `Send` but not `Sync`; wrap it in a `Mutex` to share it. `stop_handle` returns a `StopHandle` that stops
the guest from another thread, on backends that support it (not Unicorn).

## Device and host memory (Windows)

`Context::map_mmio` maps a range whose guest reads and writes call an `MmioDevice` with the offset into the range.
Devices run on the thread driving the accessing virtual CPU without the kernel lock, so they receive no `Context`.
Backends may split or merge accesses and read whole pages ahead of the guest; FEX ignores device writes.

`Context::map_host_memory` and `map_host_memory_at` alias guest memory onto a `HostMemory` buffer (zeroed, 16 KiB
pages) without copying; the emulator keeps every mapped buffer alive until it is dropped. Icicle cannot map host memory.
Both kinds of mapping are released only whole, and `restore_snapshot`/`deserialise_state` drop them, so map them again
afterwards.

## Clock, sockets and file systems (Windows)

`Settings::with_clock` replaces the host clocks with a `Clock`, and `Settings::with_socket_factory` creates the guest's
IPv4 and IPv6 stream and datagram sockets through a `SocketFactory` (returning `None` defers to the network mode's
factory). `Context::mount_provider` mounts a `FilesystemProvider` whose files are `std::io::{Read, Write, Seek}` objects;
`std::fs::File` is one. These services run on emulator threads, the clock also concurrently and while callbacks run, so
they receive no `Context` and must not use the emulator. Snapshots capture none of them: restoring keeps the clock's
current time, asks the factory for fresh sockets without their connections or data, and reopens provider files at their
saved positions without undoing the provider's changes.

## Child processes (Windows)

With `Settings::with_child_processes(ChildProcesses::new(std::env::current_exe()?))`, a guest that creates a process
gets a guest child process run by a new host process of that executable; without it, process creation fails with
`STATUS_NOT_SUPPORTED`. The executable is started with the `ChildProcesses` arguments followed by
`--sogen-managed-process <port> <token>`, so it **must handle these arguments first**, before parsing its own:

```rust,no_run
use sogen::windows::{ChildProcesses, Emulator, ManagedProcess, Settings};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::new()
        .with_emulation_root("./root")
        .with_child_processes(ChildProcesses::new(std::env::current_exe()?));
    if let Some(process) = ManagedProcess::from_arguments(std::env::args().skip(1))? {
        let mut emulator = Emulator::managed_process(&settings, process)?;
        emulator.run()?;
        return Ok(());
    }

    // The parent: create and run the emulator as usual.
    Ok(())
}
```

`from_arguments` leaves the arguments in place, so argument parsing must tolerate or strip the trailing triple. The
parent waits up to 30 seconds for the child to connect. The child's events reach the callbacks registered in the child
process, not the parent's; the child picks its backend from its own settings. The child serves the parent's requests on
its memory only while it runs, so a child that runs in slices must keep the pauses between them under the parent's
five-second request timeout. Snapshots do not capture child processes, and restoring a parent does not reconnect them.

## Tests

From `src/rust-bindings`, `cargo test -p sogen`. The Windows tests need `EMULATOR_ROOT` (an emulation root) and
`ANALYSIS_SAMPLE` (the host path of `test-sample.exe`); the Linux tests use the fixtures in `deps/sogen-linux-files`.
Tests whose fixtures are missing are skipped.
