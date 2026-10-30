use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};
use std::{env, fs, process, thread};

use sogen::linux::{
    Allocation, Application, AuxiliaryKind, Context, Emulator, FileDescriptorKind, Mount,
    OpenFlags, Parameter, Settings, Value,
};
use sogen::{
    Backend, CallContinuation, Error, Guest, HookContinuation, MemoryPermission, MemoryRegionKind,
    MemoryViolation, MemoryViolationContinuation, MemoryViolationType, StopReason,
    X86HookableInstruction, X86Register,
};

const SIGTRAP: i32 = 5;
const SIGSEGV: i32 = 11;
const SEGV_MAPERR: i32 = 1;
const TRAP_BRKPT: i32 = 1;
const CODE: u64 = 0x100000;
const DATA: u64 = 0x200000;
const FAULT_ADDRESS: u64 = 0xDEAD_0000;
const READ_WRITE: MemoryPermission = MemoryPermission::READ.union(MemoryPermission::WRITE);

struct StrtolCall {
    module: String,
    name: String,
    digits: [u8; 5],
    end: u64,
    base: i64,
}

fn settings() -> Settings {
    Settings::new().with_backend(Backend::Unicorn)
}

fn empty() -> Emulator {
    Emulator::empty(&settings()).unwrap()
}

fn with_code(code: &[u8]) -> Emulator {
    load_code(empty(), code)
}

fn load_code(emulator: Emulator, code: &[u8]) -> Emulator {
    assert!(
        emulator
            .allocate_memory_at(CODE, 0x1000, MemoryPermission::EXEC)
            .unwrap()
    );
    emulator.write_memory(CODE, code).unwrap();
    emulator.write_register(X86Register::Rip, CODE).unwrap();
    emulator
}

fn fixtures() -> Option<PathBuf> {
    let root = env::var_os("LINUX_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../deps/sogen-linux-files/test/linux")
        });

    if root.join("exit42").is_file() {
        Some(root)
    } else {
        eprintln!(
            "skipping: Linux fixtures not found at {}; set LINUX_TEST_ROOT",
            root.display()
        );
        None
    }
}

fn application(settings: &Settings, application: &Application) -> Emulator {
    Emulator::application(settings, application).unwrap()
}

fn dynamic(fixtures: &Path, name: &str) -> Emulator {
    let binary = fixtures.join("dyn-tests").join(name);
    application(
        &settings().with_emulation_root(fixtures.join("dynroot")),
        &Application::new(&binary),
    )
}

fn capture_stdout(emulator: &Emulator) -> Arc<Mutex<Vec<u8>>> {
    let output = Arc::new(Mutex::new(Vec::new()));
    let sink = output.clone();
    emulator.on_stdout(move |_, data| sink.lock().unwrap().extend_from_slice(data));
    output
}

fn temporary_root(name: &str) -> PathBuf {
    let root = env::temp_dir().join(format!("sogen-{name}-{}", process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn rip_relative(opcode: &[u8], address: u64, target: u64) -> Vec<u8> {
    let displacement = target as i64 - (address as i64 + opcode.len() as i64 + 4);
    let mut code = opcode.to_vec();
    code.extend_from_slice(&(displacement as i32).to_le_bytes());
    code
}

#[test]
fn empty_emulator_memory_registers_and_state() {
    let mut emulator = empty();
    assert_eq!(emulator.backend_name(), "Unicorn Engine");

    let exec = emulator
        .allocate_memory(&Allocation::new(0x1000, MemoryPermission::EXEC))
        .unwrap()
        .unwrap();
    let data = emulator
        .allocate_memory(&Allocation::new(0x1000, READ_WRITE))
        .unwrap()
        .unwrap();
    let write_exec = emulator
        .allocate_memory(
            &Allocation::new(0x1000, MemoryPermission::WRITE | MemoryPermission::EXEC)
                .with_start(data),
        )
        .unwrap()
        .unwrap();
    assert!(write_exec > data);
    assert_ne!(exec, data);
    assert_eq!(
        emulator.memory_region(write_exec).unwrap().permissions(),
        MemoryPermission::WRITE | MemoryPermission::EXEC
    );

    emulator.write_memory(data, b"linux-empty").unwrap();
    emulator.write_register(X86Register::Rip, exec).unwrap();
    emulator
        .write_register(X86Register::Rax, 0x1234_5678_9ABC_DEF0)
        .unwrap();
    assert_eq!(emulator.read_register(X86Register::Rip).unwrap(), exec);

    let read = |emulator: &Emulator| {
        let mut buffer = [0; 11];
        emulator.read_memory(data, &mut buffer).unwrap();
        (buffer, emulator.read_register(X86Register::Rax).unwrap())
    };

    let state = emulator.serialise_state().unwrap();
    emulator.write_memory(data, b"mutated-123").unwrap();
    emulator
        .write_register(X86Register::Rax, 0xBADF00D)
        .unwrap();
    emulator.deserialise_state(&state).unwrap();
    assert_eq!(read(&emulator), (*b"linux-empty", 0x1234_5678_9ABC_DEF0));

    assert!(matches!(
        emulator.restore_snapshot(),
        Err(Error::NoSnapshot)
    ));
    emulator.save_snapshot().unwrap();
    emulator.write_memory(data, b"snapshot-ok").unwrap();
    emulator
        .write_register(X86Register::Rax, 0xFEEDFACE)
        .unwrap();
    emulator.restore_snapshot().unwrap();
    assert_eq!(read(&emulator), (*b"linux-empty", 0x1234_5678_9ABC_DEF0));

    assert!(emulator.deserialise_state(b"garbage").is_err());
    assert_eq!(read(&emulator), (*b"linux-empty", 0x1234_5678_9ABC_DEF0));

    assert_eq!(emulator.process().thread_count(), 0);
    assert!(emulator.current_thread().is_none());
    assert!(emulator.current_thread_id().is_none());
    assert!(emulator.threads().is_empty());
    assert!(!emulator.activate_thread(999_999).unwrap());
    assert!(!emulator.perform_thread_switch().unwrap());
    assert_eq!(emulator.process().exit_status(), None);
    assert_eq!(emulator.last_stop_reason(), StopReason::None);
    assert_eq!(emulator.last_stop_detail(), "");
    assert!(matches!(
        emulator.stop_handle(),
        Err(Error::StopFromAnotherThread)
    ));
}

#[test]
fn stop_handle_stops_a_spinning_guest_from_another_thread() {
    // syscall; jmp $
    const SPIN: [u8; 4] = [0x0F, 0x05, 0xEB, 0xFE];
    let icicle = env::var_os("GHIDRA_SRC").map(|_| Backend::Icicle);
    if icicle.is_none() {
        eprintln!("skipping Icicle: GHIDRA_SRC is not set");
    }

    for backend in [Some(Backend::Fex), icicle].into_iter().flatten() {
        let emulator = match Emulator::empty(&Settings::new().with_backend(backend)) {
            Ok(emulator) => emulator,
            Err(error) => {
                eprintln!("skipping {backend:?}: {error}");
                continue;
            }
        };
        let mut emulator = load_code(emulator, &SPIN);
        let spinning = Arc::new(Barrier::new(2));
        let entered = spinning.clone();
        emulator.on_syscall(move |_, _, _| {
            entered.wait();
            HookContinuation::SkipInstruction
        });
        let stop = emulator.stop_handle().unwrap();

        let running = thread::spawn(move || {
            emulator.run().unwrap();
            emulator
        });
        spinning.wait();
        assert!(stop.stop());

        let emulator = running.join().unwrap();
        assert_eq!(emulator.last_stop_reason(), StopReason::ExplicitStop);
        assert_eq!(emulator.read_register(X86Register::Rip).unwrap(), CODE + 2);
    }
}

#[test]
fn memory_lifecycle_callbacks_observe_the_updated_mappings() {
    let emulator = empty();
    let events = Arc::new(Mutex::new(Vec::new()));
    let base = 0x300000;
    let size = 0x1000;

    let recorded = events.clone();
    emulator.on_memory_allocate(move |context, address, length, permissions, committed| {
        assert!(context.memory_region(address).is_some());
        recorded.lock().unwrap().push(format!(
            "allocate {address:#x} {length:#x} {permissions:?} {committed}"
        ));
    });
    let recorded = events.clone();
    emulator.on_memory_protect(move |context, address, length, permissions| {
        assert_eq!(
            context.memory_region(address).unwrap().permissions(),
            permissions
        );
        recorded
            .lock()
            .unwrap()
            .push(format!("protect {address:#x} {length:#x} {permissions:?}"));
    });
    let recorded = events.clone();
    emulator.on_memory_release(move |context, address, length| {
        assert!(context.memory_region(address).is_none());
        recorded
            .lock()
            .unwrap()
            .push(format!("release {address:#x} {length:#x}"));
    });

    assert!(emulator.allocate_memory_at(base, size, READ_WRITE).unwrap());
    assert!(!emulator.allocate_memory_at(base, size, READ_WRITE).unwrap());

    let region = emulator.memory_region(base + 0x80).unwrap();
    assert_eq!(region.start(), base);
    assert_eq!(region.length(), size);
    assert_eq!(region.permissions(), READ_WRITE);
    assert_eq!(region.allocation_base(), base);
    assert_eq!(region.allocation_length(), size);
    assert!(!region.is_reserved());
    assert!(region.is_committed());
    assert_eq!(region.initial_permissions(), region.permissions());
    assert_eq!(region.kind(), MemoryRegionKind::PrivateAllocation);
    assert!(emulator.memory_region(0x9000_0000).is_none());

    assert!(
        emulator
            .protect_memory(base, size, MemoryPermission::EXEC)
            .unwrap()
    );
    assert!(
        !emulator
            .protect_memory(0x9000_0000, size, MemoryPermission::READ)
            .unwrap()
    );
    assert_eq!(
        emulator.memory_region(base).unwrap().initial_permissions(),
        MemoryPermission::EXEC
    );

    let stats = emulator.memory_stats();
    assert_eq!(stats.region_count(), emulator.memory_regions().len() as u64);
    assert!(stats.mapped_bytes() >= size);
    assert!(stats.executable_bytes() >= size);

    assert!(!emulator.release_memory(0x9000_0000, 0).unwrap());
    assert!(emulator.release_memory(base, size).unwrap());
    assert!(emulator.memory_region(base).is_none());

    assert_eq!(
        *events.lock().unwrap(),
        [
            "allocate 0x300000 0x1000 MemoryPermission(READ | WRITE) true",
            "protect 0x300000 0x1000 MemoryPermission(EXEC)",
            "release 0x300000 0x1000",
        ]
    );

    let base = emulator.mmap_base();
    emulator.set_mmap_base(base + 0x100000);
    assert_eq!(emulator.mmap_base(), base + 0x100000);
    assert!(
        emulator
            .find_free_allocation_base(0x1000, None)
            .is_some_and(|address| address >= base + 0x100000)
    );
}

#[test]
fn memory_callbacks_can_cause_their_own_event() {
    let emulator = empty();
    let protections = Arc::new(Mutex::new(Vec::new()));
    let recorded = protections.clone();
    emulator.on_memory_protect(move |context, address, length, permissions| {
        recorded.lock().unwrap().push(permissions);
        if permissions.contains(MemoryPermission::EXEC) {
            assert!(
                context
                    .protect_memory(address, length, permissions - MemoryPermission::EXEC)
                    .unwrap()
            );
        }
    });

    assert!(
        emulator
            .allocate_memory_at(DATA, 0x1000, READ_WRITE)
            .unwrap()
    );
    assert!(
        emulator
            .protect_memory(
                DATA,
                0x1000,
                MemoryPermission::READ | MemoryPermission::EXEC
            )
            .unwrap()
    );
    assert_eq!(
        *protections.lock().unwrap(),
        [
            MemoryPermission::READ | MemoryPermission::EXEC,
            MemoryPermission::READ
        ]
    );
    assert_eq!(
        emulator.memory_region(DATA).unwrap().permissions(),
        MemoryPermission::READ
    );
}

#[test]
fn low_level_hooks_observe_execution_memory_and_instructions() {
    // mov rbx, 0x200000; mov rax, [rbx]; mov [rbx+8], rax; cpuid
    let mut emulator = with_code(&[
        0x48, 0xBB, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x03, 0x48, 0x89,
        0x43, 0x08, 0x0F, 0xA2,
    ]);
    assert!(
        emulator
            .allocate_memory_at(DATA, 0x1000, READ_WRITE)
            .unwrap()
    );
    emulator.write_memory(DATA, b"ABCDEFGH").unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    let execution = emulator
        .hook_memory_execution_at(CODE, move |_, address| {
            recorded
                .lock()
                .unwrap()
                .push(format!("execute {address:#x}"))
        })
        .unwrap();
    let recorded = events.clone();
    let read = emulator
        .hook_memory_read(DATA, 8, move |_, address, data| {
            recorded.lock().unwrap().push(format!(
                "read {address:#x} {}",
                String::from_utf8_lossy(data)
            ))
        })
        .unwrap();
    let recorded = events.clone();
    let write = emulator
        .hook_memory_write(DATA + 8, 8, move |_, address, data| {
            recorded.lock().unwrap().push(format!(
                "write {address:#x} {}",
                String::from_utf8_lossy(data)
            ))
        })
        .unwrap();
    let recorded = events.clone();
    let cpuid = emulator
        .hook_instruction(X86HookableInstruction::Cpuid, move |context, _| {
            recorded.lock().unwrap().push("cpuid".to_owned());
            context.stop();
            HookContinuation::SkipInstruction
        })
        .unwrap();

    emulator.run_instructions(50).unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        [
            "execute 0x100000",
            "read 0x200000 ABCDEFGH",
            "write 0x200008 ABCDEFGH",
            "cpuid"
        ]
    );
    assert_eq!(emulator.last_stop_reason(), StopReason::ExplicitStop);

    for hook in [execution, read, write, cpuid] {
        assert!(emulator.remove_hook(hook));
        assert!(!emulator.remove_hook(hook));
    }
}

#[test]
fn basic_block_hook_reports_blocks() {
    // jmp $
    let mut emulator = with_code(&[0xEB, 0xFE]);
    let blocks = Arc::new(Mutex::new(Vec::new()));
    let recorded = blocks.clone();
    emulator
        .hook_basic_block(move |context, block| {
            recorded.lock().unwrap().push(block.address());
            context.stop();
        })
        .unwrap();

    emulator.run_instructions(10).unwrap();
    assert_eq!(blocks.lock().unwrap().first(), Some(&CODE));
}

#[test]
fn interrupt_hook_runs_before_the_trap_and_can_remove_itself() {
    let mut emulator = with_code(&[0xCC]);
    let interrupts = Arc::new(Mutex::new(Vec::new()));
    let hook = Arc::new(Mutex::new(None));

    let recorded = interrupts.clone();
    let own = hook.clone();
    let id = emulator
        .hook_interrupt(move |context, interrupt| {
            recorded.lock().unwrap().push(interrupt);
            assert!(context.remove_hook(own.lock().unwrap().unwrap()));
            context.stop();
        })
        .unwrap();
    *hook.lock().unwrap() = Some(id);

    emulator.run_instructions(10).unwrap();
    assert_eq!(*interrupts.lock().unwrap(), [3]);
    assert!(!emulator.remove_hook(id));
}

#[test]
fn int3_without_a_handler_terminates_with_sigtrap() {
    let mut emulator = with_code(&[0xCC]);
    let signals = Arc::new(Mutex::new(Vec::new()));
    let recorded = signals.clone();
    emulator.on_signal(move |_, signal, _, code| recorded.lock().unwrap().push((signal, code)));

    emulator.run_instructions(10).unwrap();
    assert_eq!(
        signals.lock().unwrap().first(),
        Some(&(SIGTRAP, TRAP_BRKPT))
    );
    assert_eq!(emulator.last_stop_reason(), StopReason::SignalTermination);
    assert_eq!(emulator.process().exit_status(), Some(128 + SIGTRAP));
}

#[test]
fn unmapped_fetch_delivers_sigsegv_and_stops() {
    let mut emulator = empty();
    emulator
        .write_register(X86Register::Rip, FAULT_ADDRESS)
        .unwrap();
    let signals = Arc::new(Mutex::new(Vec::new()));
    let recorded = signals.clone();
    emulator.on_signal(move |_, signal, address, code| {
        recorded.lock().unwrap().push((signal, address, code))
    });

    emulator.run_instructions(1).unwrap();
    assert_eq!(
        signals.lock().unwrap().first(),
        Some(&(SIGSEGV, FAULT_ADDRESS, SEGV_MAPERR))
    );

    let mut emulator = empty();
    emulator
        .write_register(X86Register::Rip, FAULT_ADDRESS)
        .unwrap();
    emulator.run_instructions(1).unwrap();
    assert_eq!(
        emulator.last_stop_reason(),
        StopReason::UnhandledMemoryViolation
    );
    assert!(
        emulator
            .last_stop_detail()
            .contains(&format!("{FAULT_ADDRESS:#x}"))
    );
    assert_eq!(emulator.process().exit_status(), None);
}

#[test]
fn memory_violation_callbacks_and_hooks_can_stop_the_guest() {
    let mut emulator = empty();
    emulator
        .write_register(X86Register::Rip, FAULT_ADDRESS)
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    emulator.on_memory_violate(move |_, violation| {
        recorded
            .lock()
            .unwrap()
            .push((violation.address(), violation.kind()));
        MemoryViolationContinuation::Stop
    });
    emulator.run_instructions(1).unwrap();
    assert_eq!(
        events.lock().unwrap().first(),
        Some(&(FAULT_ADDRESS, MemoryViolationType::Unmapped))
    );
    assert_eq!(
        emulator.last_stop_reason(),
        StopReason::UnhandledMemoryViolation
    );

    let mut emulator = empty();
    emulator
        .write_register(X86Register::Rip, FAULT_ADDRESS)
        .unwrap();
    let hook = Arc::new(Mutex::new(None));
    let own = hook.clone();
    let id = emulator
        .hook_memory_violation(move |context, violation| {
            assert_eq!(violation.address(), FAULT_ADDRESS);
            assert!(context.remove_hook(own.lock().unwrap().unwrap()));
            MemoryViolationContinuation::Stop
        })
        .unwrap();
    *hook.lock().unwrap() = Some(id);
    emulator.run_instructions(1).unwrap();
    assert_eq!(
        emulator.last_stop_reason(),
        StopReason::UnhandledMemoryViolation
    );
    assert!(!emulator.remove_hook(id));
}

#[test]
fn data_violations_report_the_operation() {
    for (opcode, operation, use_hook) in [
        ([0x48, 0x8B, 0x05], MemoryPermission::READ, false),
        ([0x48, 0x89, 0x05], MemoryPermission::WRITE, true),
    ] {
        let mut emulator = with_code(&rip_relative(&opcode, CODE, DATA));
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = events.clone();
        let handler = move |_: &Context, violation: &MemoryViolation| {
            recorded
                .lock()
                .unwrap()
                .push((violation.address(), violation.operation()));
            MemoryViolationContinuation::Stop
        };
        if use_hook {
            emulator.hook_memory_violation(handler).unwrap();
        } else {
            emulator.on_memory_violate(handler);
        }

        emulator.run_instructions(10).unwrap();
        assert_eq!(events.lock().unwrap().first(), Some(&(DATA, operation)));
        assert_eq!(
            emulator.last_stop_reason(),
            StopReason::UnhandledMemoryViolation
        );
        assert_eq!(emulator.process().exit_status(), None);
    }
}

#[test]
fn memory_violation_recovery_maps_the_missing_page() {
    for (base, continuation) in [
        (0x410000, MemoryViolationContinuation::Resume),
        (0x400000, MemoryViolationContinuation::Restart),
    ] {
        let mut emulator = empty();
        emulator.write_register(X86Register::Rip, base).unwrap();
        let faults = Arc::new(Mutex::new(Vec::new()));
        let cpuids = Arc::new(Mutex::new(0));

        let recorded = faults.clone();
        emulator.on_memory_violate(move |context, violation| {
            recorded.lock().unwrap().push(violation.address());
            assert!(
                context
                    .allocate_memory_at(base, 0x1000, MemoryPermission::EXEC)
                    .unwrap()
            );
            context.write_memory(base, [0x0F, 0xA2]).unwrap();
            continuation
        });
        let counted = cpuids.clone();
        emulator
            .hook_instruction(X86HookableInstruction::Cpuid, move |context, _| {
                *counted.lock().unwrap() += 1;
                context.stop();
                HookContinuation::SkipInstruction
            })
            .unwrap();

        emulator.run_instructions(10).unwrap();
        assert_eq!(faults.lock().unwrap().first(), Some(&base));
        assert_eq!(*cpuids.lock().unwrap(), 1, "{continuation:?} did not retry");
    }
}

#[test]
fn step_out_reports_missing_frames() {
    let mut emulator = empty();
    emulator.write_register(X86Register::Rbp, 0).unwrap();
    assert!(matches!(emulator.step_out(), Err(Error::ZeroFramePointer)));

    emulator.write_register(X86Register::Rbp, 0x300000).unwrap();
    assert!(matches!(
        emulator.step_out(),
        Err(Error::ReturnAddressUnreadable(0x300008))
    ));

    assert!(
        emulator
            .allocate_memory_at(0x300000, 0x1000, READ_WRITE)
            .unwrap()
    );
    assert!(matches!(
        emulator.step_out(),
        Err(Error::ReturnAddressZero(0x300008))
    ));
}

#[test]
fn breakpoints_stop_and_are_stepped_over_when_resuming() {
    // cpuid; cpuid; cpuid
    let mut emulator = with_code(&[0x0F, 0xA2, 0x0F, 0xA2, 0x0F, 0xA2]);
    emulator.set_breakpoint(CODE + 2).unwrap();
    emulator.set_breakpoint(CODE + 4).unwrap();
    emulator.set_breakpoint(CODE + 4).unwrap();
    assert_eq!(emulator.breakpoints(), [CODE + 2, CODE + 4]);

    emulator.continue_execution().unwrap();
    assert_eq!(emulator.last_stop_reason(), StopReason::Breakpoint);
    assert_eq!(emulator.last_stop_detail(), "address=0x100002");
    let registers = emulator.registers().unwrap();
    assert_eq!(registers.get(X86Register::Rip), Some(CODE + 2));
    assert!(registers.get(X86Register::Cs).is_some_and(|cs| cs != 0));

    emulator.continue_execution().unwrap();
    assert_eq!(emulator.last_stop_reason(), StopReason::Breakpoint);
    assert_eq!(emulator.read_register(X86Register::Rip).unwrap(), CODE + 4);

    emulator.step_into().unwrap();
    assert_eq!(emulator.read_register(X86Register::Rip).unwrap(), CODE + 6);
    assert_eq!(emulator.breakpoints(), [CODE + 2, CODE + 4]);

    let instructions = emulator.disassemble(CODE, 3).unwrap();
    assert_eq!(
        instructions
            .iter()
            .map(|instruction| (instruction.address(), instruction.mnemonic()))
            .collect::<Vec<_>>(),
        [(CODE, "cpuid"), (CODE + 2, "cpuid"), (CODE + 4, "cpuid")]
    );
    assert_eq!(emulator.disassemble(CODE + 0xFFE, 10).unwrap().len(), 1);
    assert!(emulator.disassemble(FAULT_ADDRESS, 4).unwrap().is_empty());

    assert!(emulator.clear_breakpoint(CODE + 2));
    assert!(emulator.clear_breakpoint(CODE + 4));
    assert!(!emulator.clear_breakpoint(CODE + 4));
    assert!(emulator.breakpoints().is_empty());
}

#[test]
fn step_over_runs_a_whole_call() {
    // call +1; cpuid; ret  (the callee is the ret at CODE + 7, after the cpuid at CODE + 5)
    let mut emulator = with_code(&[0xE8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0xA2, 0xC3]);
    assert!(
        emulator
            .allocate_memory_at(DATA, 0x1000, READ_WRITE)
            .unwrap()
    );
    emulator
        .write_register(X86Register::Rsp, DATA + 0x800)
        .unwrap();

    let call = emulator.disassemble(CODE, 1).unwrap();
    assert!(call[0].is_call());
    emulator.set_breakpoint(CODE).unwrap();

    emulator.step_over().unwrap();
    assert_eq!(emulator.read_register(X86Register::Rip).unwrap(), CODE + 5);
    assert_eq!(
        emulator.read_register(X86Register::Rsp).unwrap(),
        DATA + 0x800
    );
    assert_eq!(emulator.breakpoints(), [CODE]);

    emulator.step_over().unwrap();
    assert_eq!(emulator.read_register(X86Register::Rip).unwrap(), CODE + 7);

    emulator.write_register(X86Register::Rip, CODE).unwrap();
    emulator.step_into().unwrap();
    assert_eq!(emulator.read_register(X86Register::Rip).unwrap(), CODE + 7);
}

#[test]
fn port_mappings_are_validated_and_queryable() {
    assert!(matches!(
        Emulator::empty(&settings().with_port_mapping(0, 8080)),
        Err(Error::ZeroPort)
    ));

    let emulator = Emulator::empty(&settings().with_port_mapping(80, 8080)).unwrap();
    assert_eq!(emulator.host_port(80), Some(8080));
    assert_eq!(emulator.emulator_port(8080), Some(80));
    assert_eq!(emulator.host_port(81), None);

    emulator.map_port(80, 9090).unwrap();
    assert_eq!(emulator.host_port(80), Some(9090));
    assert_eq!(emulator.emulator_port(8080), None);
    assert!(matches!(emulator.map_port(80, 0), Err(Error::ZeroPort)));
}

#[test]
fn exit_code_and_thread_termination() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let mut emulator = application(&settings(), &Application::new(fixtures.join("exit42")));
    let thread = emulator.current_thread().unwrap();
    assert!(thread.is_active());
    assert_eq!(
        thread.current_ip(),
        emulator.read_register(X86Register::Rip).unwrap()
    );
    assert_eq!(emulator.thread(thread.id()), Some(thread));
    assert!(emulator.activate_thread(thread.id()).unwrap());

    let terminated = Arc::new(Mutex::new(Vec::new()));
    let recorded = terminated.clone();
    emulator.on_thread_terminated(move |_, thread| {
        recorded
            .lock()
            .unwrap()
            .push((thread.id(), thread.is_terminated(), thread.exit_code()))
    });

    let emulator = thread::spawn(move || {
        emulator.run().unwrap();
        emulator
    })
    .join()
    .unwrap();
    assert_eq!(emulator.process().exit_status(), Some(42));
    assert_eq!(emulator.last_stop_reason(), StopReason::NormalExit);
    assert_eq!(*terminated.lock().unwrap(), [(thread.id(), true, 42)]);
    assert!(emulator.current_thread().is_none());
    assert_eq!(emulator.process().thread_count(), 1);
}

#[test]
fn stdout_and_syscalls_are_reported() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let mut emulator = application(&settings(), &Application::new(fixtures.join("hello")));
    let stdout = capture_stdout(&emulator);
    let syscalls = Arc::new(Mutex::new(Vec::new()));
    let recorded = syscalls.clone();
    emulator.on_syscall(move |_, id, name| {
        recorded.lock().unwrap().push((id, name.to_owned()));
        HookContinuation::RunInstruction
    });

    emulator.run().unwrap();
    assert_eq!(*stdout.lock().unwrap(), b"Hello World!\n");
    assert_eq!(
        *syscalls.lock().unwrap(),
        [(1, "write".to_owned()), (60, "exit".to_owned())]
    );
    assert_eq!(emulator.process().exit_status(), Some(0));
    assert!(emulator.executed_instructions() > 0);
}

#[test]
fn skipped_syscalls_do_not_run() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let mut emulator = application(&settings(), &Application::new(fixtures.join("hello")));
    let stdout = capture_stdout(&emulator);
    emulator.on_syscall(|context, id, _| {
        if id == 1 {
            context.write_register(X86Register::Rax, 13).unwrap();
            HookContinuation::SkipInstruction
        } else {
            HookContinuation::RunInstruction
        }
    });

    emulator.run().unwrap();
    assert!(stdout.lock().unwrap().is_empty());
    assert_eq!(emulator.process().exit_status(), Some(0));
}

#[test]
fn process_modules_single_step_and_state_restore() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let mut emulator = application(&settings(), &Application::new(fixtures.join("multitest")));
    let ids = emulator.process();
    assert_eq!((ids.pid(), ids.ppid(), ids.uid()), (1, 1, 1000));

    let modules = emulator.modules();
    let main = modules
        .iter()
        .find(|module| module.name().contains("multitest"))
        .unwrap()
        .clone();
    assert!(main.contains(main.entry_point()));
    assert!(main.needed_libraries().is_empty());
    assert_eq!(
        emulator.find_module_by_address(main.entry_point()),
        Some(main.clone())
    );
    assert_eq!(
        emulator.find_module_by_name(main.name()),
        Some(main.clone())
    );

    let section = modules
        .iter()
        .flat_map(|module| module.sections())
        .find(|section| section.permissions() == MemoryPermission::READ | MemoryPermission::EXEC)
        .unwrap();
    assert_eq!(
        emulator
            .memory_region(section.start())
            .unwrap()
            .permissions(),
        MemoryPermission::READ | MemoryPermission::EXEC
    );

    let replayed = Arc::new(Mutex::new(Vec::new()));
    let recorded = replayed.clone();
    emulator
        .on_module_load(move |_, module| recorded.lock().unwrap().push(module.name().to_owned()));
    assert!(replayed.lock().unwrap().contains(&main.name().to_owned()));

    let state = emulator.serialise_state().unwrap();
    let mmap_base = emulator.mmap_base();
    emulator.set_mmap_base(mmap_base + 0x100000);
    emulator.deserialise_state(&state).unwrap();
    assert_eq!(emulator.mmap_base(), mmap_base);
    assert_eq!(emulator.modules().len(), modules.len());

    let rip = emulator.read_register(X86Register::Rip).unwrap();
    let executed = emulator.executed_instructions();
    emulator.run_instructions(1).unwrap();
    assert_ne!(emulator.read_register(X86Register::Rip).unwrap(), rip);
    assert_eq!(emulator.executed_instructions(), executed + 1);
    assert_eq!(emulator.last_stop_reason(), StopReason::InstructionLimit);
    assert_eq!(emulator.process().exit_status(), None);

    let stdout = capture_stdout(&emulator);
    emulator.run().unwrap();
    assert_eq!(emulator.process().exit_status(), Some(0));
    assert!(String::from_utf8_lossy(&stdout.lock().unwrap()).contains("Linux"));
}

#[test]
fn snapshots_rerun_the_program() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let mut emulator = application(&settings(), &Application::new(fixtures.join("hello")));
    let stdout = capture_stdout(&emulator);
    emulator.save_snapshot().unwrap();

    emulator.run().unwrap();
    emulator.restore_snapshot().unwrap();
    assert_eq!(emulator.process().exit_status(), None);
    emulator.run().unwrap();

    assert_eq!(*stdout.lock().unwrap(), b"Hello World!\nHello World!\n");
    assert_eq!(emulator.process().exit_status(), Some(0));
}

#[test]
fn signal_handlers_recover_and_mprotect_is_observed() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let mut emulator = application(&settings(), &Application::new(fixtures.join("test_signal")));
    let stdout = capture_stdout(&emulator);
    let signals = Arc::new(Mutex::new(Vec::new()));
    let recorded = signals.clone();
    emulator.on_signal(move |_, signal, _, _| recorded.lock().unwrap().push(signal));
    emulator.run().unwrap();
    assert_eq!(emulator.process().exit_status(), Some(0));
    assert!(signals.lock().unwrap().contains(&SIGSEGV));
    assert!(String::from_utf8_lossy(&stdout.lock().unwrap()).contains("Signal recovery OK"));

    let mut emulator = application(
        &settings(),
        &Application::new(fixtures.join("test_advanced")),
    );
    let stdout = capture_stdout(&emulator);
    let protections = Arc::new(Mutex::new(0));
    let counted = protections.clone();
    emulator.on_memory_protect(move |_, _, _, _| *counted.lock().unwrap() += 1);
    emulator.run().unwrap();
    assert_eq!(emulator.process().exit_status(), Some(0));
    assert!(*protections.lock().unwrap() > 0);
    assert!(String::from_utf8_lossy(&stdout.lock().unwrap()).contains("All tests OK!"));
}

#[test]
fn dynamic_programs_get_arguments_environment_and_runtime_modules() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let binary = fixtures.join("dyn-tests/hello_dyn");
    let mut emulator = application(
        &settings().with_emulation_root(fixtures.join("dynroot")),
        &Application::new(&binary)
            .with_argument("first")
            .with_argument("second"),
    );
    let stdout = capture_stdout(&emulator);
    let main = emulator
        .modules()
        .into_iter()
        .find(|module| module.name().contains("hello_dyn"))
        .unwrap();
    assert!(!main.needed_libraries().is_empty());
    assert_eq!(
        emulator.find_module_by_address(main.entry_point()),
        Some(main.clone())
    );

    let loaded = Arc::new(Mutex::new(Vec::new()));
    let recorded = loaded.clone();
    emulator
        .on_module_load(move |_, module| recorded.lock().unwrap().push(module.name().to_owned()));
    let before = loaded.lock().unwrap().len();

    emulator.run().unwrap();
    let output = String::from_utf8(stdout.lock().unwrap().clone()).unwrap();
    assert!(output.contains("argc = 3"), "{output}");
    assert!(output.contains("argv[2] = second"), "{output}");
    assert_eq!(emulator.process().exit_status(), Some(0));
    assert_eq!(loaded.lock().unwrap().len(), emulator.modules().len());
    assert!(before <= emulator.modules().len());

    let mut emulator = application(
        &settings().with_emulation_root(fixtures.join("dynroot")),
        &Application::new(fixtures.join("dyn-tests/test_dyn_features"))
            .with_environment_variable("PATH", "/custom/bin"),
    );
    let stdout = capture_stdout(&emulator);
    emulator.run().unwrap();
    let output = String::from_utf8(stdout.lock().unwrap().clone()).unwrap();
    assert!(output.contains("PATH=/custom/bin"), "{output}");
    assert_eq!(emulator.process().exit_status(), Some(0));
}

#[test]
fn symbol_hooks_decode_arguments_and_intercept() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let mut emulator = dynamic(&fixtures, "test_dyn_features");
    let stdout = capture_stdout(&emulator);
    for key in ["libc!", "!strtol"] {
        assert!(matches!(
            emulator.hook_symbol(key, &[], |_, _, _| CallContinuation::RunOriginal),
            Err(Error::HookKey(_))
        ));
    }

    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    emulator
        .hook_symbol(
            "strtol",
            &[Parameter::Pointer, Parameter::Pointer, Parameter::I32],
            move |context, call, arguments| {
                let [
                    Value::Pointer(text),
                    Value::Pointer(end),
                    Value::Signed(base),
                ] = *arguments
                else {
                    panic!("unexpected arguments {arguments:?}");
                };
                let mut digits = [0; 5];
                context.read_memory(text, &mut digits).unwrap();
                assert_ne!(call.return_address(), 0);
                recorded.lock().unwrap().push(StrtolCall {
                    module: call.module().name().to_owned(),
                    name: call.name().to_owned(),
                    digits,
                    end,
                    base,
                });
                CallContinuation::RunOriginal
            },
        )
        .unwrap();

    let intercepted = Arc::new(Mutex::new(0));
    let counted = intercepted.clone();
    emulator
        .hook_symbol("atoi", &[Parameter::Pointer], move |_, call, _| {
            *counted.lock().unwrap() += 1;
            call.set_return_value(7);
            CallContinuation::Intercept
        })
        .unwrap();

    let state = emulator.serialise_state().unwrap();
    emulator.deserialise_state(&state).unwrap();
    emulator.run().unwrap();

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "strtol");
    assert_eq!(calls[0].digits, *b"12345");
    assert_eq!((calls[0].end, calls[0].base), (0, 10));
    assert_eq!(*intercepted.lock().unwrap(), 1);
    let output = String::from_utf8(stdout.lock().unwrap().clone()).unwrap();
    assert!(output.contains("strtol: 12345"), "{output}");
    assert!(output.contains("atoi: 7"), "{output}");
    assert!(output.contains("FAIL: atoi"), "{output}");
    assert_eq!(emulator.process().exit_status(), Some(1));

    let module = calls[0].module.clone();
    let mut emulator = dynamic(&fixtures, "test_dyn_features");
    let stdout = capture_stdout(&emulator);
    emulator
        .hook_symbol(&format!("{module}!atoi"), &[], |_, call, _| {
            call.set_return_value(42);
            CallContinuation::Intercept
        })
        .unwrap();
    emulator
        .hook_symbol("missing!atoi", &[], |_, _, _| {
            panic!("wrong module matched")
        })
        .unwrap();
    emulator.save_snapshot().unwrap();
    emulator.restore_snapshot().unwrap();
    emulator.run().unwrap();
    assert!(
        String::from_utf8_lossy(&stdout.lock().unwrap())
            .contains("All dynamic linking tests passed!")
    );
    assert_eq!(emulator.process().exit_status(), Some(0));
}

#[test]
fn symbol_hooks_can_remove_themselves_and_narrow_values() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let mut emulator = dynamic(&fixtures, "hello_dyn");
    let stdout = capture_stdout(&emulator);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    emulator
        .hook_symbol(
            "printf",
            &[
                Parameter::Pointer,
                Parameter::I8,
                Parameter::U8,
                Parameter::I16,
                Parameter::Bool,
                Parameter::Char,
                Parameter::Raw,
            ],
            move |context, call, arguments| {
                recorded.lock().unwrap().push(arguments[1..6].to_vec());
                assert!(context.unhook_symbol("printf"));
                assert!(!context.unhook_symbol("printf"));
                call.set_return_value(0);
                CallContinuation::RunOriginal
            },
        )
        .unwrap();
    emulator
        .hook_symbol("puts", &[], |_, call, _| {
            call.set_return_value(1);
            CallContinuation::Intercept
        })
        .unwrap();
    emulator.set_breakpoint(0x4005BD).unwrap();

    emulator.run().unwrap();
    assert_eq!(emulator.last_stop_reason(), StopReason::Breakpoint);
    emulator
        .write_register(X86Register::Rsi, 0xFFFF_FFFF_8001_01FF)
        .unwrap();
    emulator.write_register(X86Register::Rdx, 0x1FE).unwrap();
    emulator.write_register(X86Register::Rcx, 0x1_8000).unwrap();
    emulator.write_register(X86Register::R8, 0x100).unwrap();
    emulator.write_register(X86Register::R9, 0x141).unwrap();
    emulator.continue_execution().unwrap();

    assert_eq!(
        *calls.lock().unwrap(),
        [vec![
            Value::Signed(-1),
            Value::Unsigned(0xFE),
            Value::Signed(-0x8000),
            Value::Bool(false),
            Value::Char(0x41),
        ]]
    );
    let output = String::from_utf8(stdout.lock().unwrap().clone()).unwrap();
    assert!(!output.contains("Hello from"), "{output}");
    assert!(output.contains("argv[0] = "), "{output}");
    assert_eq!(emulator.process().exit_status(), Some(0));
}

#[test]
fn debugger_steps_over_calls_and_out_of_functions() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    const MAIN: u64 = 0x400595;
    const CALL_PUTS: u64 = 0x4005A9;
    const AFTER_PUTS: u64 = 0x4005AE;

    let mut emulator = dynamic(&fixtures, "hello_dyn");
    let stdout = capture_stdout(&emulator);
    emulator.set_breakpoint(MAIN).unwrap();
    emulator.continue_execution().unwrap();
    assert_eq!(emulator.read_register(X86Register::Rip).unwrap(), MAIN);

    emulator.run_to(CALL_PUTS).unwrap();
    assert_eq!(emulator.last_stop_reason(), StopReason::Breakpoint);
    let call = emulator.disassemble(CALL_PUTS, 1).unwrap();
    assert_eq!(call[0].mnemonic(), "call");
    assert!(call[0].is_call());

    let frames = emulator.call_stack().unwrap();
    assert_eq!(frames[0].instruction_pointer(), CALL_PUTS);
    assert!(
        frames[0]
            .module()
            .is_some_and(|module| module.contains("hello_dyn"))
    );
    assert!(frames.len() >= 2);
    let caller = frames[1].instruction_pointer();

    emulator.step_over().unwrap();
    assert_eq!(
        emulator.read_register(X86Register::Rip).unwrap(),
        AFTER_PUTS
    );

    emulator.step_out().unwrap();
    assert_eq!(emulator.read_register(X86Register::Rip).unwrap(), caller);
    assert_eq!(
        frames[1].module(),
        emulator
            .find_module_by_address(caller)
            .map(|module| module.name().to_owned())
            .as_deref()
    );

    emulator.continue_execution().unwrap();
    assert_eq!(emulator.process().exit_status(), Some(0));
    let output = String::from_utf8(stdout.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("Hello from") && output.contains("argv[0]"),
        "{output}"
    );
    assert_eq!(emulator.breakpoints(), [MAIN]);
}

#[test]
fn periodic_event_fires_every_0x20000_instructions() {
    let mut emulator = with_code(&[0xEB, 0xFE]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    emulator.on_periodic_event(move |context| {
        recorded
            .lock()
            .unwrap()
            .push(context.executed_instructions())
    });

    emulator.run_instructions(0x60000).unwrap();
    assert_eq!(*events.lock().unwrap(), [0x20000, 0x40000, 0x60000]);

    emulator.clear_on_periodic_event();
    emulator.run_instructions(0x20000).unwrap();
    assert_eq!(events.lock().unwrap().len(), 3);
}

#[test]
fn file_descriptors_list_standard_streams_and_opened_files() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let root = temporary_root("descriptors");
    let mut emulator = application(
        &settings().with_emulation_root(&root),
        &Application::new(fixtures.join("torture/bin/openat_getdents_torture")),
    );
    let stdout = capture_stdout(&emulator);
    let snapshot = Arc::new(Mutex::new(Vec::new()));
    let recorded = snapshot.clone();
    emulator.on_syscall(move |context, id, _| {
        let number = context.read_register(X86Register::Rdi).unwrap() as u32;
        let mut recorded = recorded.lock().unwrap();
        if id == 3
            && recorded.is_empty()
            && context
                .file_descriptor(number)
                .is_some_and(|descriptor| descriptor.kind() == FileDescriptorKind::Directory)
        {
            *recorded = context.file_descriptors();
        }
        HookContinuation::RunInstruction
    });
    emulator.run().unwrap();
    assert_eq!(emulator.process().exit_status(), Some(0));
    assert!(String::from_utf8_lossy(&stdout.lock().unwrap()).contains("ALL PASS"));

    let descriptors = snapshot.lock().unwrap().clone();
    for (number, path) in ["/dev/stdin", "/dev/stdout", "/dev/stderr"]
        .into_iter()
        .enumerate()
    {
        let descriptor = &descriptors[number];
        assert_eq!(descriptor.number(), number as u32);
        assert_eq!(descriptor.kind(), FileDescriptorKind::File);
        assert_eq!(descriptor.host_path(), Some(Path::new(path)));
    }
    assert_eq!(descriptors[1].flags(), OpenFlags::WRONLY);

    let file = descriptors
        .iter()
        .find(|descriptor| {
            descriptor
                .guest_path()
                .is_some_and(|path| path.ends_with("a.txt"))
        })
        .unwrap();
    assert_eq!(file.kind(), FileDescriptorKind::File);
    assert!(
        file.flags()
            .contains(OpenFlags::RDWR | OpenFlags::CREAT | OpenFlags::TRUNC)
    );
    assert!(file.host_path().unwrap().starts_with(root.join("tmp")));
    assert!(
        file.host_path()
            .unwrap()
            .ends_with(file.guest_path().unwrap().strip_prefix("/").unwrap())
    );

    let directory = descriptors
        .iter()
        .find(|descriptor| descriptor.kind() == FileDescriptorKind::Directory)
        .unwrap();
    assert!(directory.flags().contains(OpenFlags::DIRECTORY));
    assert_eq!(
        Some(directory.guest_path().unwrap()),
        file.guest_path().unwrap().parent()
    );
    assert!(emulator.file_descriptor(file.number()).is_none());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn mounts_and_passthrough_prefixes_translate_guest_paths() {
    let root = temporary_root("mounts");
    let emulator = Emulator::empty(
        &settings()
            .with_emulation_root(&root)
            .with_mount(Mount::new("/sogen-host/data", "/mnt/data").with_read_only(true)),
    )
    .unwrap();

    let mounted = PathBuf::from("/sogen-host/data/file.txt");
    assert_eq!(
        emulator.translate_path("/mnt/data/file.txt").unwrap(),
        mounted
    );
    assert_eq!(
        emulator
            .translate_relative_path("/mnt", "data/./file.txt")
            .unwrap(),
        mounted
    );
    assert!(emulator.translate_path("/mnt/data\0/file.txt").is_err());
    assert!(emulator.is_path_read_only("/mnt/data/file.txt"));
    assert_eq!(
        emulator.translate_path("/etc/passwd").unwrap(),
        root.join("etc/passwd")
    );
    assert!(!emulator.is_path_read_only("/etc/passwd"));

    emulator
        .add_mount(&Mount::new("/sogen-host/scratch", "/scratch"))
        .unwrap();
    assert_eq!(
        emulator.translate_path("/scratch/out").unwrap(),
        Path::new("/sogen-host/scratch/out")
    );
    assert!(!emulator.is_path_read_only("/scratch/out"));

    emulator
        .add_passthrough_prefix(Path::new("/sogen-host/libraries"))
        .unwrap();
    assert_eq!(
        emulator
            .translate_path("/sogen-host/libraries/libc.so")
            .unwrap(),
        Path::new("/sogen-host/libraries/libc.so")
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn process_reports_the_launched_application() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    let binary = fixtures.join("dyn-tests/hello_dyn");
    let emulator = application(
        &settings().with_emulation_root(fixtures.join("dynroot")),
        &Application::new(&binary)
            .with_argument("first")
            .with_argument("second")
            .with_environment_variable("GREETING", "hello=world")
            .with_working_directory("/tmp"),
    );

    let process = emulator.process();
    assert_eq!(
        process.arguments(),
        [binary.to_str().unwrap(), "first", "second"]
    );
    let environment = process
        .environment()
        .iter()
        .map(|variable| (variable.name(), variable.value()))
        .collect::<Vec<_>>();
    assert_eq!(environment, [("GREETING", "hello=world")]);
    assert_eq!(process.working_directory(), Path::new("/tmp"));
    assert_eq!(process.executable_path().file_name(), binary.file_name());
    assert!(process.brk_base() != 0 && process.brk_current() >= process.brk_base());

    let main = emulator
        .modules()
        .into_iter()
        .find(|module| module.name().contains("hello_dyn"))
        .unwrap();
    let auxiliary = emulator.auxiliary_vector();
    let value = |kind| {
        auxiliary
            .iter()
            .find(|entry| entry.kind() == kind)
            .map(|entry| entry.value())
    };
    assert_eq!(value(AuxiliaryKind::Entry), Some(main.entry_point()));
    assert_eq!(value(AuxiliaryKind::Pagesz), Some(0x1000));
    assert_eq!(value(AuxiliaryKind::Uid), Some(u64::from(process.uid())));

    let libc = emulator.resolve_library("libc.so", &main).unwrap();
    assert_eq!(
        fs::canonicalize(&libc).unwrap(),
        fs::canonicalize(fixtures.join("dynroot/lib/libc.so")).unwrap()
    );
    assert_eq!(emulator.resolve_library("libmissing.so", &main), None);

    let empty = empty();
    let loaded = Arc::new(Mutex::new(Vec::new()));
    let recorded = loaded.clone();
    empty.on_module_load(move |_, module| recorded.lock().unwrap().push(module.image_base()));
    let module = empty.map_local_module(&libc).unwrap();
    assert_eq!(module.path(), libc);
    assert_eq!(*loaded.lock().unwrap(), [module.image_base()]);
    assert_eq!(
        empty.find_module_by_address(module.image_base()),
        Some(module)
    );
}

#[test]
fn thread_registers_round_trip_for_a_cloned_thread() {
    let Some(fixtures) = fixtures() else {
        return;
    };

    const CLONE_THREAD: u32 = 0x10000;
    const CLONE_CHILD_CLEARTID: u32 = 0x200000;
    const RBX: u64 = 0x1122_3344_5566_7788;
    let child_stack = DATA + 0x1000;

    let mut code = vec![0x48, 0xBB];
    code.extend_from_slice(&RBX.to_le_bytes());
    code.extend_from_slice(&[0xB8, 56, 0, 0, 0, 0xBF]);
    code.extend_from_slice(&(CLONE_THREAD | CLONE_CHILD_CLEARTID).to_le_bytes());
    code.extend_from_slice(&[0x48, 0xBE]);
    code.extend_from_slice(&child_stack.to_le_bytes());
    code.extend_from_slice(&[0x31, 0xD2, 0x49, 0xBA]);
    code.extend_from_slice(&DATA.to_le_bytes());
    code.extend_from_slice(&[0x45, 0x31, 0xC0, 0x0F, 0x05, 0xEB, 0xFE]);

    let mut emulator = application(&settings(), &Application::new(fixtures.join("exit42")));
    assert!(
        emulator
            .allocate_memory_at(
                CODE,
                0x1000,
                MemoryPermission::READ | MemoryPermission::EXEC
            )
            .unwrap()
    );
    assert!(
        emulator
            .allocate_memory_at(DATA, 0x1000, READ_WRITE)
            .unwrap()
    );
    emulator.write_memory(CODE, &code).unwrap();
    emulator.write_register(X86Register::Rip, CODE).unwrap();

    let created = Arc::new(Mutex::new(None));
    let recorded = created.clone();
    emulator.on_thread_create(move |context, thread| {
        *recorded.lock().unwrap() = Some(thread.id());
        context.stop();
    });
    emulator.run().unwrap();

    let child = created.lock().unwrap().unwrap();
    let thread = emulator.thread(child).unwrap();
    assert!(!thread.is_active());
    assert_eq!(thread.clear_child_tid(), DATA);

    let mut registers = emulator.thread_registers(child).unwrap();
    assert_eq!(registers.get(X86Register::Rax), Some(0));
    assert_eq!(registers.get(X86Register::Rsp), Some(child_stack));
    assert_eq!(registers.get(X86Register::Rbx), Some(RBX));
    assert_eq!(registers.get(X86Register::Cs), None);
    assert!(!registers.set(X86Register::Cs, 0));

    assert!(registers.set(X86Register::Rbx, 0xFEED));
    assert!(registers.set(X86Register::Rip, CODE));
    assert!(emulator.set_thread_registers(child, &registers).unwrap());
    assert_eq!(emulator.thread_registers(child), Some(registers));
    assert_eq!(emulator.thread(child).unwrap().current_ip(), CODE);

    let active = emulator.current_thread_id().unwrap();
    let mut live = emulator.thread_registers(active).unwrap();
    assert_eq!(
        live.get(X86Register::Rip),
        Some(emulator.read_register(X86Register::Rip).unwrap())
    );
    assert!(live.set(X86Register::R12, 0xBEEF));
    assert!(emulator.set_thread_registers(active, &live).unwrap());
    assert_eq!(emulator.read_register(X86Register::R12).unwrap(), 0xBEEF);

    assert_eq!(emulator.thread_registers(u32::MAX), None);
    assert!(!emulator.set_thread_registers(u32::MAX, &live).unwrap());
}
