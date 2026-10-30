use std::sync::{Arc, Mutex};

use sogen::arm64::Emulator;
use sogen::{
    Arm64HookableInstruction, Arm64Register, DescriptorTable, Error, Guest, HookContinuation,
    MemoryPermission, X86Register, X86Segment,
};

const CODE: u64 = 0x10000;

fn emulator(instructions: &[u32]) -> Emulator {
    let emulator = Emulator::new(None).unwrap();
    emulator
        .map_memory(
            CODE,
            0x1000,
            MemoryPermission::READ | MemoryPermission::WRITE | MemoryPermission::EXEC,
        )
        .unwrap();
    let code = instructions
        .iter()
        .flat_map(|instruction| instruction.to_le_bytes())
        .collect::<Vec<_>>();
    emulator.write_memory(CODE, code).unwrap();
    emulator.write_register(Arm64Register::Pc, CODE).unwrap();
    emulator
}

#[test]
fn single_step_and_saved_registers() {
    // mov x0, #42; add x0, x0, #1
    let mut emulator = emulator(&[0xD2800540, 0x91000400]);
    assert!(emulator.supports_instruction_counting());

    emulator.run_instructions(1).unwrap();
    assert_eq!(emulator.read_register(Arm64Register::Pc).unwrap(), CODE + 4);
    assert_eq!(emulator.read_register(Arm64Register::X0).unwrap(), 42);

    let saved = emulator.save_registers().unwrap();
    emulator.run_instructions(1).unwrap();
    assert_eq!(emulator.read_register(Arm64Register::X0).unwrap(), 43);

    emulator.restore_registers(&saved).unwrap();
    assert_eq!(emulator.read_register(Arm64Register::X0).unwrap(), 42);
    assert_eq!(emulator.read_register(Arm64Register::Pc).unwrap(), CODE + 4);
}

#[test]
fn svc_hook_sees_the_guest_and_skips_the_instruction() {
    // svc #7; add x0, x0, #1; svc #7
    let mut emulator = emulator(&[0xD40000E1, 0x91000400, 0xD40000E1]);
    let calls = Arc::new(Mutex::new(Vec::new()));

    let recorded = calls.clone();
    let hook = emulator
        .hook_instruction(Arm64HookableInstruction::Svc, move |guest, immediate| {
            assert_eq!(guest.read_register(Arm64Register::Pc).unwrap(), CODE);
            recorded.lock().unwrap().push(immediate);
            guest.write_register(Arm64Register::X0, 99).unwrap();
            HookContinuation::SkipInstruction
        })
        .unwrap();

    emulator.run_instructions(1).unwrap();
    assert_eq!(*calls.lock().unwrap(), [7]);
    assert_eq!(emulator.read_register(Arm64Register::X0).unwrap(), 99);

    emulator.run_instructions(1).unwrap();
    assert_eq!(emulator.read_register(Arm64Register::X0).unwrap(), 100);

    assert!(emulator.remove_hook(hook));
    assert!(!emulator.remove_hook(hook));
    emulator
        .hook_instruction(Arm64HookableInstruction::Svc, |_, _| {
            HookContinuation::SkipInstruction
        })
        .unwrap();
    emulator.run_instructions(1).unwrap();
    assert_eq!(*calls.lock().unwrap(), [7]);
    assert_eq!(
        emulator.read_register(Arm64Register::Pc).unwrap(),
        CODE + 12
    );
}

#[test]
fn execution_hooks_report_every_instruction() {
    // mov x0, #42; add x0, x0, #1
    let mut emulator = emulator(&[0xD2800540, 0x91000400]);
    let executed = Arc::new(Mutex::new(Vec::new()));

    let recorded = executed.clone();
    emulator
        .hook_memory_execution(move |_, address| recorded.lock().unwrap().push(address))
        .unwrap();
    emulator.run_instructions(2).unwrap();

    assert_eq!(*executed.lock().unwrap(), [CODE, CODE + 4]);
}

#[test]
fn invalid_accesses_fail() {
    let emulator = emulator(&[0xD4000021]);
    let mut buffer = [0; 8];

    assert!(matches!(
        emulator.read_memory(0x90000, &mut buffer),
        Err(Error::Native(_))
    ));
    assert!(!emulator.try_read_memory(0x90000, &mut buffer));
    assert!(emulator.restore_registers(b"invalid").is_err());
    assert!(emulator.read_register(X86Register::Rax).is_err());
    assert!(emulator.segment_base(X86Segment::Fs).is_err());
    assert!(emulator.set_segment_base(X86Segment::Gs, 0).is_err());
    assert!(emulator.descriptor_table(DescriptorTable::Gdt).is_err());
    assert!(emulator.load_gdt(0, 0).is_err());
    assert!(emulator.unmap_memory(CODE, 0x1000).unwrap());
}
