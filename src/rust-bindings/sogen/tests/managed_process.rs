use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{env, thread};

use sogen::windows::{
    Allocation, Application, ChildProcesses, Emulator, ManagedProcess, ObjectInfo, Settings,
};
use sogen::{Guest, HookContinuation, MemoryPermission};

const CMD: &str = r"C:\Windows\System32\cmd.exe";
const RUN_IN_SLICES: &str = "--run-in-slices";
const SLICE_INSTRUCTIONS: u64 = 50_000;
const PAUSE: Duration = Duration::from_millis(100);
const CHILD_ALLOCATIONS: u32 = 8;
const ALLOCATION_INTERVAL: Duration = Duration::from_millis(50);
const PAUSED_SLICES: u32 = 2 * CHILD_ALLOCATIONS;

/// The test binary is also the host process of every guest child process, so it handles the managed-process
/// arguments before anything else.
fn main() {
    let Some(root) = env::var_os("EMULATOR_ROOT").map(PathBuf::from) else {
        eprintln!("skipping: EMULATOR_ROOT is not set");
        return;
    };

    let settings = Settings::new().with_emulation_root(root);
    if let Some(process) = ManagedProcess::from_arguments(env::args().skip(1)).unwrap() {
        let application = process.application();
        assert!(application.path().to_ascii_lowercase().ends_with("cmd.exe"));
        let mut emulator = Emulator::managed_process(&settings, process).unwrap();
        if env::args().nth(1).as_deref() == Some(RUN_IN_SLICES) {
            run_in_slices(&mut emulator);
        } else {
            emulator.run().unwrap();
        }
        return;
    }

    let executable = env::current_exe().unwrap();
    guest_child_process_reports_its_exit_status(
        &settings
            .clone()
            .with_child_processes(ChildProcesses::new(&executable)),
    );
    println!("test guest_child_process_reports_its_exit_status ... ok");
    parent_memory_requests_wait_for_the_child_to_run(
        &settings
            .with_child_processes(ChildProcesses::new(executable).with_argument(RUN_IN_SLICES)),
    );
    println!("test parent_memory_requests_wait_for_the_child_to_run ... ok");
}

/// Pauses after each of the first short runs, checking that guest memory stays unchanged while the emulator is not
/// running, then runs to the end.
fn run_in_slices(emulator: &mut Emulator) {
    for _ in 0..PAUSED_SLICES {
        emulator.run_instructions(SLICE_INSTRUCTIONS).unwrap();
        if emulator.process().exit_status().is_some() {
            return;
        }

        let regions = emulator.memory_regions();
        thread::sleep(PAUSE);
        assert_eq!(
            emulator.memory_regions(),
            regions,
            "the parent changed guest memory between runs"
        );
    }

    emulator.run().unwrap();
}

/// A thread body for an `argument` pointing at a process handle, a 32-bit success count and a relative
/// `NtDelayExecution` interval: `count` times, it waits through `delay`, then allocates a committed page in the
/// process through `allocate` and counts the success.
fn allocation_loop(delay: u64, allocate: u64, count: u32) -> Vec<u8> {
    let mut code = vec![0x53, 0x56, 0x48, 0x83, 0xEC, 0x48, 0x48, 0x89, 0xCE, 0xBB];
    code.extend(count.to_le_bytes());
    let start = code.len();
    code.extend([0x31, 0xC9, 0x48, 0x8D, 0x56, 0x10, 0x48, 0xB8]);
    code.extend(delay.to_le_bytes());
    code.extend([
        0xFF, 0xD0, 0x48, 0xC7, 0x44, 0x24, 0x30, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x44, 0x24,
        0x38, 0x00, 0x10, 0x00, 0x00, 0x48, 0x8B, 0x0E, 0x48, 0x8D, 0x54, 0x24, 0x30, 0x45, 0x31,
        0xC0, 0x4C, 0x8D, 0x4C, 0x24, 0x38, 0xC7, 0x44, 0x24, 0x20, 0x00, 0x30, 0x00, 0x00, 0xC7,
        0x44, 0x24, 0x28, 0x04, 0x00, 0x00, 0x00, 0x48, 0xB8,
    ]);
    code.extend(allocate.to_le_bytes());
    code.extend([
        0xFF, 0xD0, 0x85, 0xC0, 0x75, 0x03, 0xFF, 0x46, 0x08, 0xFF, 0xCB, 0x75,
    ]);
    let back = i8::try_from(start as isize - (code.len() + 1) as isize).unwrap();
    code.push(back as u8);
    code.extend([0x48, 0x83, 0xC4, 0x48, 0x5E, 0x5B, 0x31, 0xC0, 0xC3]);
    code
}

fn exit_seven() -> Application {
    Application::new(CMD)
        .with_argument("/c")
        .with_argument(CMD)
        .with_argument("/c")
        .with_argument("exit 7")
}

fn guest_child_process_reports_its_exit_status(settings: &Settings) {
    let mut emulator = Emulator::application(settings, &exit_seven()).unwrap();
    emulator.run().unwrap();
    assert_eq!(emulator.process().exit_status(), Some(7));
}

/// While the child pauses between runs, a parent thread allocates in it now and then; each allocation must wait for
/// a run and then succeed.
fn parent_memory_requests_wait_for_the_child_to_run(settings: &Settings) {
    let mut emulator = Emulator::application(settings, &exit_seven()).unwrap();
    emulator.setup_process_if_necessary().unwrap();
    let ntdll = emulator.find_module_by_name("ntdll.dll").unwrap();
    let export = |name| {
        ntdll
            .exports()
            .iter()
            .find(|export| export.name() == name)
            .unwrap()
            .address()
    };
    let code = emulator
        .allocate_memory(&Allocation::new(
            0x1000,
            MemoryPermission::READ | MemoryPermission::WRITE | MemoryPermission::EXEC,
        ))
        .unwrap()
        .unwrap();
    let data = code + 0x800;
    emulator
        .write_memory(
            code,
            allocation_loop(
                export("NtDelayExecution"),
                export("NtAllocateVirtualMemory"),
                CHILD_ALLOCATIONS,
            ),
        )
        .unwrap();
    let interval = i64::try_from(ALLOCATION_INTERVAL.as_nanos() / 100).unwrap();
    emulator
        .write_memory(data + 16, (-interval).to_le_bytes())
        .unwrap();

    let parent = emulator.process().process_id();
    let started = AtomicBool::new(false);
    emulator.on_syscall(move |context, _, name| {
        if name != "NtResumeThread" || started.load(Ordering::Relaxed) {
            return HookContinuation::RunInstruction;
        }

        let child = context
            .handles()
            .into_iter()
            .find(|handle| matches!(handle.object(), ObjectInfo::Process { id } if *id != parent));
        if let Some(child) = child {
            started.store(true, Ordering::Relaxed);
            context
                .write_memory(data, child.value().to_le_bytes())
                .unwrap();
            context.create_thread(code, data, false).unwrap();
        }
        HookContinuation::RunInstruction
    });
    emulator.run().unwrap();

    assert_eq!(emulator.process().exit_status(), Some(7));
    let mut allocations = [0; 4];
    emulator.read_memory(data + 8, &mut allocations).unwrap();
    assert_eq!(u32::from_le_bytes(allocations), CHILD_ALLOCATIONS);
}
