use std::cell::RefCell;
use std::collections::BTreeMap;
use std::mem;
use std::sync::Arc;

use super::{Context, Module};
use crate::error::Error;
use crate::machine::{CallContinuation, Guest, HookId, X86Register, parse_hook_key};

const ARGUMENT_REGISTERS: [X86Register; 6] = [
    X86Register::Rdi,
    X86Register::Rsi,
    X86Register::Rdx,
    X86Register::Rcx,
    X86Register::R8,
    X86Register::R9,
];

/// How a System V AMD64 argument of a hooked symbol is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Parameter {
    Bool,
    /// The low byte as [`Value::Char`].
    Char,
    I16,
    I32,
    I64,
    I8,
    /// A guest address as [`Value::Pointer`], not dereferenced.
    Pointer,
    /// The full 64-bit argument slot as [`Value::Unsigned`].
    Raw,
    U16,
    U32,
    U64,
    U8,
}

impl Parameter {
    fn decode(self, slot: u64) -> Value {
        match self {
            Self::Raw | Self::U64 => Value::Unsigned(slot),
            Self::Bool => Value::Bool(slot as u8 != 0),
            Self::Char => Value::Char(slot as u8),
            Self::I8 => Value::Signed(i64::from(slot as i8)),
            Self::U8 => Value::Unsigned(u64::from(slot as u8)),
            Self::I16 => Value::Signed(i64::from(slot as i16)),
            Self::U16 => Value::Unsigned(u64::from(slot as u16)),
            Self::I32 => Value::Signed(i64::from(slot as i32)),
            Self::U32 => Value::Unsigned(u64::from(slot as u32)),
            Self::I64 => Value::Signed(slot as i64),
            Self::Pointer => Value::Pointer(slot),
        }
    }
}

/// A decoded argument, narrowed and sign-extended according to its [`Parameter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Value {
    Bool(bool),
    Char(u8),
    Pointer(u64),
    Signed(i64),
    Unsigned(u64),
}

/// A call of a hooked symbol, observed at the symbol's first instruction.
#[derive(Debug, Clone)]
pub struct SymbolCall {
    module: Arc<Module>,
    name: Arc<str>,
    address: u64,
    return_address: u64,
    stack_pointer: u64,
    return_value: u64,
}

impl SymbolCall {
    /// The module exporting the symbol, as it was when the hook was installed.
    pub fn module(&self) -> &Module {
        &self.module
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn address(&self) -> u64 {
        self.address
    }

    pub fn return_address(&self) -> u64 {
        self.return_address
    }

    pub fn stack_pointer(&self) -> u64 {
        self.stack_pointer
    }

    /// The value placed in RAX when the callback returns [`CallContinuation::Intercept`]; zero by default.
    pub fn return_value(&self) -> u64 {
        self.return_value
    }

    pub fn set_return_value(&mut self, value: u64) {
        self.return_value = value;
    }
}

type SymbolCallback = dyn Fn(&Context, &mut SymbolCall, &[Value]) -> CallContinuation + Send + Sync;

struct SymbolTarget {
    module: Option<String>,
    name: Arc<str>,
    parameters: Vec<Parameter>,
    callback: Box<SymbolCallback>,
}

impl SymbolTarget {
    fn matches(&self, module: &Module) -> bool {
        self.module.as_deref().is_none_or(|filter| {
            filter == module.name()
                || module.path().file_stem().and_then(|stem| stem.to_str()) == Some(filter)
        })
    }

    /// Hooks every matching export of `modules`, removing the hooks already installed if one fails.
    fn install(
        self: &Arc<Self>,
        context: &Context,
        modules: &[Arc<Module>],
    ) -> Result<Vec<HookId>, Error> {
        let mut hooks = Vec::new();
        for module in modules.iter().filter(|module| self.matches(module)) {
            for symbol in module.exports() {
                if symbol.name() != &*self.name || symbol.address() == 0 {
                    continue;
                }

                let target = self.clone();
                let module = module.clone();
                let address = symbol.address();
                let hook = context.hook_memory_execution_at(address, move |context, _| {
                    if let Err(error) = context.symbols.dispatch(context, &target, &module, address)
                    {
                        context.fail(error);
                    }
                });
                match hook {
                    Ok(hook) => hooks.push(hook),
                    Err(error) => {
                        for hook in hooks {
                            context.remove_hook(hook);
                        }
                        return Err(error);
                    }
                }
            }
        }

        Ok(hooks)
    }

    fn dispatch(
        &self,
        context: &Context,
        module: &Arc<Module>,
        address: u64,
        arguments: &mut Vec<Value>,
    ) -> Result<(), Error> {
        let stack_pointer = context.read_register(X86Register::Rsp)?;
        let Some(return_address) = context.read_stack_slot(stack_pointer) else {
            return Ok(());
        };

        arguments.clear();
        for (index, parameter) in self.parameters.iter().enumerate() {
            let slot = match ARGUMENT_REGISTERS.get(index) {
                Some(&register) => context.read_register(register)?,
                None => {
                    let offset = (index - ARGUMENT_REGISTERS.len() + 1) as u64 * 8;
                    let Some(slot) = context.read_stack_slot(stack_pointer.wrapping_add(offset))
                    else {
                        return Ok(());
                    };
                    slot
                }
            };
            arguments.push(parameter.decode(slot));
        }

        let mut call = SymbolCall {
            module: module.clone(),
            name: self.name.clone(),
            address,
            return_address,
            stack_pointer,
            return_value: 0,
        };
        if (self.callback)(context, &mut call, arguments) == CallContinuation::Intercept {
            context.write_register(X86Register::Rax, call.return_value)?;
            context.write_register(X86Register::Rsp, stack_pointer.wrapping_add(8))?;
            context.write_register(X86Register::Rip, return_address)?;
        }

        Ok(())
    }
}

struct SymbolHook {
    target: Arc<SymbolTarget>,
    hooks: Vec<HookId>,
}

/// The symbol hooks of an emulator, keyed by `name` or `module!name`, with the execution hooks installed for each.
#[derive(Default)]
pub(super) struct SymbolHooks {
    entries: RefCell<BTreeMap<Arc<str>, SymbolHook>>,
    arguments: RefCell<Vec<Value>>,
}

impl SymbolHooks {
    pub(super) fn hook(
        &self,
        context: &Context,
        key: &str,
        parameters: &[Parameter],
        callback: impl Fn(&Context, &mut SymbolCall, &[Value]) -> CallContinuation
        + Send
        + Sync
        + 'static,
    ) -> Result<(), Error> {
        let (module, name) = parse_hook_key(key)?;
        let target = Arc::new(SymbolTarget {
            module: module.map(str::to_owned),
            name: Arc::from(name),
            parameters: parameters.to_vec(),
            callback: Box::new(callback),
        });
        let modules = context
            .modules()
            .into_iter()
            .map(Arc::new)
            .collect::<Vec<_>>();
        let hooks = target.install(context, &modules)?;

        let previous = self
            .entries
            .borrow_mut()
            .insert(Arc::from(key), SymbolHook { target, hooks });
        for hook in previous.into_iter().flat_map(|previous| previous.hooks) {
            context.remove_hook(hook);
        }
        Ok(())
    }

    pub(super) fn unhook(&self, context: &Context, key: &str) -> bool {
        let removed = self.entries.borrow_mut().remove(key);
        let Some(removed) = removed else {
            return false;
        };

        for hook in removed.hooks {
            context.remove_hook(hook);
        }
        true
    }

    pub(super) fn clear(&self, context: &Context) {
        let entries = mem::take(&mut *self.entries.borrow_mut());
        for hook in entries.into_values().flat_map(|entry| entry.hooks) {
            context.remove_hook(hook);
        }
    }

    /// Installing hooks never runs a callback, so the entries stay borrowed while they are updated.
    pub(super) fn install(&self, context: &Context, module: Module) -> Result<(), Error> {
        let modules = [Arc::new(module)];
        for entry in self.entries.borrow_mut().values_mut() {
            let hooks = entry.target.install(context, &modules)?;
            entry.hooks.extend(hooks);
        }
        Ok(())
    }

    /// Replaces every entry's hooks with hooks for the current modules. An entry whose new hooks cannot be installed
    /// keeps its previous ones.
    pub(super) fn rebuild(&self, context: &Context) -> Result<(), Error> {
        let modules = context
            .modules()
            .into_iter()
            .map(Arc::new)
            .collect::<Vec<_>>();
        for entry in self.entries.borrow_mut().values_mut() {
            let hooks = entry.target.install(context, &modules)?;
            for hook in mem::replace(&mut entry.hooks, hooks) {
                context.remove_hook(hook);
            }
        }
        Ok(())
    }

    fn dispatch(
        &self,
        context: &Context,
        target: &SymbolTarget,
        module: &Arc<Module>,
        address: u64,
    ) -> Result<(), Error> {
        let mut arguments = mem::take(&mut *self.arguments.borrow_mut());
        let result = target.dispatch(context, module, address, &mut arguments);
        *self.arguments.borrow_mut() = arguments;
        result
    }
}
