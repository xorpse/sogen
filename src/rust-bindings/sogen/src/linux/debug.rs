use std::array;
use std::cell::RefCell;
use std::collections::BTreeMap;

use sogen_sys::linux::ffi;

use super::Context;
use crate::error::Error;
use crate::machine::{Guest, HookId, StopReason, X86Register};

const MAX_DISASSEMBLY: usize = 4096;
const MAX_INSTRUCTION_SIZE: usize = 16;
const MAX_STACK_FRAMES: usize = 64;
const PAGE_SIZE: u64 = 0x1000;

const DEBUG_REGISTERS: [X86Register; 24] = [
    X86Register::Rax,
    X86Register::Rbx,
    X86Register::Rcx,
    X86Register::Rdx,
    X86Register::Rsi,
    X86Register::Rdi,
    X86Register::Rbp,
    X86Register::Rsp,
    X86Register::R8,
    X86Register::R9,
    X86Register::R10,
    X86Register::R11,
    X86Register::R12,
    X86Register::R13,
    X86Register::R14,
    X86Register::R15,
    X86Register::Rip,
    X86Register::Rflags,
    X86Register::Cs,
    X86Register::Ss,
    X86Register::Ds,
    X86Register::Es,
    X86Register::Fs,
    X86Register::Gs,
];

const THREAD_REGISTERS: [X86Register; 20] = [
    X86Register::Rax,
    X86Register::Rbx,
    X86Register::Rcx,
    X86Register::Rdx,
    X86Register::Rsi,
    X86Register::Rdi,
    X86Register::Rbp,
    X86Register::Rsp,
    X86Register::R8,
    X86Register::R9,
    X86Register::R10,
    X86Register::R11,
    X86Register::R12,
    X86Register::R13,
    X86Register::R14,
    X86Register::R15,
    X86Register::Rip,
    X86Register::Rflags,
    X86Register::FsBase,
    X86Register::GsBase,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterValue {
    register: X86Register,
    value: u64,
}

impl RegisterValue {
    pub fn register(&self) -> X86Register {
        self.register
    }

    pub fn value(&self) -> u64 {
        self.value
    }
}

/// The general purpose, flags and segment registers of the active thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registers {
    values: [RegisterValue; DEBUG_REGISTERS.len()],
}

impl Registers {
    pub(super) fn read(context: &Context) -> Result<Self, Error> {
        let mut values = DEBUG_REGISTERS.map(|register| RegisterValue { register, value: 0 });
        for entry in &mut values {
            entry.value = context.read_register(entry.register)?;
        }
        Ok(Self { values })
    }

    pub fn get(&self, register: X86Register) -> Option<u64> {
        register_value(&self.values, register)
    }

    pub fn iter(&self) -> impl Iterator<Item = RegisterValue> + '_ {
        self.values.iter().copied()
    }
}

/// The registers the Linux personality keeps for a thread while it is switched out: the general purpose registers,
/// RIP, RFLAGS and the FS and GS bases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadRegisters {
    values: [RegisterValue; THREAD_REGISTERS.len()],
}

impl ThreadRegisters {
    pub(super) fn from_native(registers: ffi::LinuxThreadRegisters) -> Self {
        let ffi::LinuxThreadRegisters {
            rax,
            rbx,
            rcx,
            rdx,
            rsi,
            rdi,
            rbp,
            rsp,
            r8,
            r9,
            r10,
            r11,
            r12,
            r13,
            r14,
            r15,
            rip,
            rflags,
            fs_base,
            gs_base,
        } = registers;
        let values = [
            rax, rbx, rcx, rdx, rsi, rdi, rbp, rsp, r8, r9, r10, r11, r12, r13, r14, r15, rip,
            rflags, fs_base, gs_base,
        ];
        Self {
            values: array::from_fn(|index| RegisterValue {
                register: THREAD_REGISTERS[index],
                value: values[index],
            }),
        }
    }

    pub(super) fn to_native(&self) -> ffi::LinuxThreadRegisters {
        let [
            rax,
            rbx,
            rcx,
            rdx,
            rsi,
            rdi,
            rbp,
            rsp,
            r8,
            r9,
            r10,
            r11,
            r12,
            r13,
            r14,
            r15,
            rip,
            rflags,
            fs_base,
            gs_base,
        ] = self.values.map(|entry| entry.value);
        ffi::LinuxThreadRegisters {
            rax,
            rbx,
            rcx,
            rdx,
            rsi,
            rdi,
            rbp,
            rsp,
            r8,
            r9,
            r10,
            r11,
            r12,
            r13,
            r14,
            r15,
            rip,
            rflags,
            fs_base,
            gs_base,
        }
    }

    pub fn get(&self, register: X86Register) -> Option<u64> {
        register_value(&self.values, register)
    }

    /// Returns false if the register is not one the thread keeps.
    pub fn set(&mut self, register: X86Register, value: u64) -> bool {
        let Some(entry) = self
            .values
            .iter_mut()
            .find(|entry| entry.register == register)
        else {
            return false;
        };
        entry.value = value;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = RegisterValue> + '_ {
        self.values.iter().copied()
    }
}

fn register_value(values: &[RegisterValue], register: X86Register) -> Option<u64> {
    values
        .iter()
        .find(|entry| entry.register == register)
        .map(|entry| entry.value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    address: u64,
    bytes: Vec<u8>,
    mnemonic: String,
    operands: String,
    call: bool,
}

impl Instruction {
    pub(super) fn from_native(instruction: ffi::LinuxInstruction) -> Self {
        Self {
            address: instruction.address,
            bytes: instruction.bytes,
            mnemonic: instruction.mnemonic,
            operands: instruction.operands,
            call: instruction.call,
        }
    }

    pub fn address(&self) -> u64 {
        self.address
    }

    pub fn size(&self) -> u64 {
        self.bytes.len() as u64
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn mnemonic(&self) -> &str {
        &self.mnemonic
    }

    pub fn operands(&self) -> &str {
        &self.operands
    }

    pub fn is_call(&self) -> bool {
        self.call
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    instruction_pointer: u64,
    stack_pointer: u64,
    module: Option<String>,
}

impl StackFrame {
    pub fn instruction_pointer(&self) -> u64 {
        self.instruction_pointer
    }

    /// RSP for the innermost frame, the frame pointer of the frame for callers.
    pub fn stack_pointer(&self) -> u64 {
        self.stack_pointer
    }

    pub fn module(&self) -> Option<&str> {
        self.module.as_deref()
    }
}

/// Breakpoints and stepping. A breakpoint stops the guest before the instruction at its address runs, with
/// [`StopReason::Breakpoint`]; resuming steps over the breakpoint the guest is stopped at.
#[derive(Default)]
pub(super) struct Debugger {
    breakpoints: RefCell<BTreeMap<u64, HookId>>,
}

impl Debugger {
    pub(super) fn set_breakpoint(&self, context: &Context, address: u64) -> Result<(), Error> {
        if self.breakpoints.borrow().contains_key(&address) {
            return Ok(());
        }

        let hook = context.hook_memory_execution_at(address, move |context, _| {
            break_at(context, address);
        })?;
        self.breakpoints.borrow_mut().insert(address, hook);
        Ok(())
    }

    pub(super) fn clear_breakpoint(&self, context: &Context, address: u64) -> bool {
        let hook = self.breakpoints.borrow_mut().remove(&address);
        hook.is_some_and(|hook| context.remove_hook(hook))
    }

    pub(super) fn breakpoints(&self) -> Vec<u64> {
        self.breakpoints.borrow().keys().copied().collect()
    }

    pub(super) fn step_into(&self, context: &Context) -> Result<(), Error> {
        if self.step_past_breakpoint(context)? {
            return Ok(());
        }

        context.start(1)
    }

    pub(super) fn step_over(&self, context: &Context) -> Result<(), Error> {
        let address = context.read_register(X86Register::Rip)?;
        match disassemble(context, address, 1)?.into_iter().next() {
            Some(instruction) if instruction.is_call() => {
                let frame = context.read_register(X86Register::Rsp)?;
                self.run_until(
                    context,
                    address.wrapping_add(instruction.size()),
                    Some(frame),
                )
            }
            _ => self.step_into(context),
        }
    }

    pub(super) fn step_out(&self, context: &Context) -> Result<(), Error> {
        let frame_pointer = context.read_register(X86Register::Rbp)?;
        if frame_pointer == 0 {
            return Err(Error::ZeroFramePointer);
        }

        let slot = frame_pointer.wrapping_add(8);
        match context.read_stack_slot(slot) {
            None => Err(Error::ReturnAddressUnreadable(slot)),
            Some(0) => Err(Error::ReturnAddressZero(slot)),
            Some(return_address) => self.run_until(context, return_address, None),
        }
    }

    pub(super) fn resume(&self, context: &Context) -> Result<(), Error> {
        if self.step_past_breakpoint(context)?
            && context.last_stop_reason() != StopReason::InstructionLimit
        {
            return Ok(());
        }

        context.start(0)
    }

    pub(super) fn run_until(
        &self,
        context: &Context,
        address: u64,
        outer_frame: Option<u64>,
    ) -> Result<(), Error> {
        let hook = context.hook_memory_execution_at(address, move |context, _| {
            let returned = match outer_frame {
                None => true,
                Some(frame) => match context.read_register(X86Register::Rsp) {
                    Ok(stack_pointer) => stack_pointer >= frame,
                    Err(error) => return context.fail(error),
                },
            };
            if returned {
                break_at(context, address);
            }
        })?;
        let result = self.resume(context);
        context.remove_hook(hook);
        result
    }

    fn step_past_breakpoint(&self, context: &Context) -> Result<bool, Error> {
        let address = context.read_register(X86Register::Rip)?;
        if !self.clear_breakpoint(context, address) {
            return Ok(false);
        }

        let result = context.start(1);
        self.set_breakpoint(context, address)?;
        result.map(|()| true)
    }
}

fn break_at(context: &Context, address: u64) {
    context
        .native
        .record_stop(StopReason::Breakpoint, &format!("address={address:#x}"));
    match context.write_register(X86Register::Rip, address) {
        Ok(()) => context.stop(),
        Err(error) => context.fail(error),
    }
}

/// Decodes up to `count` instructions (at most 4096) from the readable bytes at `address`.
pub(super) fn disassemble(
    context: &Context,
    address: u64,
    count: usize,
) -> Result<Vec<Instruction>, Error> {
    let count = count.min(MAX_DISASSEMBLY);
    let size = count * MAX_INSTRUCTION_SIZE;
    let mut bytes = vec![0; size];
    let mut read = 0;
    while read < size {
        let current = address.wrapping_add(read as u64);
        let chunk = ((PAGE_SIZE - current % PAGE_SIZE) as usize).min(size - read);
        if !context.try_read_memory(current, &mut bytes[read..read + chunk]) {
            break;
        }
        read += chunk;
    }
    if read == 0 {
        return Ok(Vec::new());
    }

    let instructions = context
        .native
        .disassemble(address, &bytes[..read], count as u64)?;
    Ok(instructions
        .into_iter()
        .map(Instruction::from_native)
        .collect())
}

/// The current frame followed by the callers found by walking the RBP chain. The walk ends at a zero return address
/// or once the saved frame pointer no longer moves up the stack.
pub(super) fn call_stack(context: &Context) -> Result<Vec<StackFrame>, Error> {
    let module_name = |address| {
        context
            .find_module_by_address(address)
            .map(|module| module.name().to_owned())
    };

    let instruction_pointer = context.read_register(X86Register::Rip)?;
    let mut frames = vec![StackFrame {
        instruction_pointer,
        stack_pointer: context.read_register(X86Register::Rsp)?,
        module: module_name(instruction_pointer),
    }];

    let mut frame_pointer = context.read_register(X86Register::Rbp)?;
    while frame_pointer != 0 && frames.len() <= MAX_STACK_FRAMES {
        let (Some(saved_frame_pointer), Some(return_address)) = (
            context.read_stack_slot(frame_pointer),
            context.read_stack_slot(frame_pointer.wrapping_add(8)),
        ) else {
            break;
        };
        if return_address == 0 {
            break;
        }

        frames.push(StackFrame {
            instruction_pointer: return_address,
            stack_pointer: frame_pointer,
            module: module_name(return_address),
        });
        if saved_frame_pointer <= frame_pointer {
            break;
        }
        frame_pointer = saved_frame_pointer;
    }

    Ok(frames)
}
