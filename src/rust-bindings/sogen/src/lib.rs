#![doc = include_str!("../README.md")]

pub mod arm64;
mod error;
pub mod linux;
mod machine;
pub mod windows;

pub use error::Error;
pub use machine::{
    Arm64HookableInstruction, Arm64Register, Backend, BasicBlock, CallContinuation,
    DescriptorTable, DescriptorTableRegister, Guest, HookContinuation, HookId, HookableInstruction,
    LogLevel, MemoryExecutionHookMode, MemoryPermission, MemoryRegionKind, MemoryViolation,
    MemoryViolationContinuation, MemoryViolationType, Register, StopHandle, StopReason,
    X86HookableInstruction, X86Register, X86Segment,
};
