use std::cell::RefCell;
use std::collections::BTreeMap;
use std::mem;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use sogen_sys::windows::ffi::{CallingConvention, CodeMode, NamedAddress};

use super::Context;
use crate::error::Error;
use crate::machine::{Arm64Register, CallContinuation, Guest, HookId, parse_hook_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApiSignature {
    convention: CallingConvention,
    parameter_count: usize,
}

impl ApiSignature {
    /// `convention` applies to 32-bit code; 64-bit x86 code always uses the Windows x64 convention and ARM64 code
    /// its ABI.
    pub fn new(convention: CallingConvention, parameter_count: usize) -> Self {
        Self {
            convention,
            parameter_count,
        }
    }

    pub fn convention(&self) -> CallingConvention {
        self.convention
    }

    pub fn parameter_count(&self) -> usize {
        self.parameter_count
    }
}

/// A call of a hooked API, seen at its first instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiCall {
    module: Arc<str>,
    name: Arc<str>,
    address: u64,
    return_address: u64,
    return_value: u64,
}

impl ApiCall {
    /// The module whose export was hit, which for a forwarder can differ from the module the caller imported.
    pub fn module(&self) -> &str {
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

    pub fn return_value(&self) -> u64 {
        self.return_value
    }

    /// The value the caller receives when the hook returns [`CallContinuation::Intercept`].
    pub fn set_return_value(&mut self, return_value: u64) {
        self.return_value = return_value;
    }
}

type ApiCallback = dyn Fn(&Context, &mut ApiCall, &[u64]) -> CallContinuation + Send + Sync;

struct ApiHookEntry {
    module: Option<String>,
    name: Arc<str>,
    signature: ApiSignature,
    callback: Box<ApiCallback>,
}

impl ApiHookEntry {
    fn matches_module(&self, module: &str) -> bool {
        let Some(expected) = &self.module else {
            return true;
        };

        let name = module.to_ascii_lowercase();
        let stem = name.rfind('.').map_or(name.as_str(), |dot| &name[..dot]);
        *expected == name || expected == stem
    }
}

#[derive(Clone)]
struct ApiHookHit {
    key: Arc<str>,
    entry: Arc<ApiHookEntry>,
    module: Arc<str>,
}

#[derive(Default)]
struct ApiHookState {
    entries: BTreeMap<Arc<str>, Arc<ApiHookEntry>>,
    index: FxHashMap<u64, Arc<[ApiHookHit]>>,
    address_hooks: FxHashMap<u64, HookId>,
    dispatch_depth: usize,
    retired_hooks: FxHashMap<u64, HookId>,
    arguments: Vec<u64>,
}

impl ApiHookState {
    fn is_registered(&self, hit: &ApiHookHit) -> bool {
        self.entries
            .get(&*hit.key)
            .is_some_and(|entry| Arc::ptr_eq(entry, &hit.entry))
    }

    fn release_address(&mut self, context: &Context, address: u64) {
        let Some(hook) = self.address_hooks.remove(&address) else {
            return;
        };

        if self.dispatch_depth != 0 {
            self.retired_hooks.insert(address, hook);
        } else {
            context.remove_hook(hook);
        }
    }
}

/// API hooks keyed by `name` or `module!name`, with one execution hook per hooked address.
///
/// Execution hooks a dispatch makes obsolete are removed only once every dispatch has returned, so a callback can
/// replace or remove any entry, including its own, while its hook is running.
#[derive(Default)]
pub(super) struct ApiHooks {
    state: RefCell<ApiHookState>,
}

impl ApiHooks {
    pub(super) fn insert(
        &self,
        context: &Context,
        key: &str,
        signature: ApiSignature,
        callback: impl Fn(&Context, &mut ApiCall, &[u64]) -> CallContinuation + Send + Sync + 'static,
    ) -> Result<(), Error> {
        let (module, name) = parse_hook_key(key)?;
        let entry = Arc::new(ApiHookEntry {
            module: module.map(str::to_ascii_lowercase),
            name: Arc::from(name),
            signature,
            callback: Box::new(callback),
        });
        self.rebuild(context, Some((Arc::from(key), entry)), None)
    }

    pub(super) fn remove(&self, context: &Context, key: &str) -> bool {
        let mut state = self.state.borrow_mut();
        let Some(entry) = state.entries.remove(key) else {
            return false;
        };

        let mut emptied = Vec::new();
        for (&address, hits) in &mut state.index {
            if !hits.iter().any(|hit| Arc::ptr_eq(&hit.entry, &entry)) {
                continue;
            }

            *hits = hits
                .iter()
                .filter(|hit| !Arc::ptr_eq(&hit.entry, &entry))
                .cloned()
                .collect();
            if hits.is_empty() {
                emptied.push(address);
            }
        }

        for address in emptied {
            state.index.remove(&address);
            state.release_address(context, address);
        }

        true
    }

    pub(super) fn clear(&self, context: &Context) {
        let mut state = self.state.borrow_mut();
        state.entries.clear();
        state.index.clear();
        let hooks = mem::take(&mut state.address_hooks);
        if state.dispatch_depth != 0 {
            state.retired_hooks.extend(hooks);
            return;
        }

        for hook in hooks.into_values() {
            context.remove_hook(hook);
        }
    }

    /// Rebuilds the address index from the mapped modules, leaving out the module at `unloading`.
    pub(super) fn refresh(&self, context: &Context, unloading: Option<u64>) -> Result<(), Error> {
        self.rebuild(context, None, unloading)
    }

    /// Indexes the entries, with `added` replacing any entry under its key, and installs the execution hooks the index
    /// needs. Nothing changes if one of them cannot be installed. Installing hooks never runs a callback, so the state
    /// stays borrowed throughout.
    fn rebuild(
        &self,
        context: &Context,
        added: Option<(Arc<str>, Arc<ApiHookEntry>)>,
        unloading: Option<u64>,
    ) -> Result<(), Error> {
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        let replaced = added.as_ref().map(|(key, _)| key);
        let entries = state
            .entries
            .iter()
            .filter(|&(key, _)| Some(key) != replaced)
            .chain(added.as_ref().map(|(key, entry)| (key, entry)));
        let mut index = FxHashMap::<u64, Vec<ApiHookHit>>::default();
        let mut named = FxHashMap::<&str, Vec<NamedAddress>>::default();
        for (key, entry) in entries {
            let addresses = named
                .entry(&*entry.name)
                .or_insert_with(|| context.native.named_addresses(&entry.name));
            for address in addresses.iter() {
                if Some(address.image_base) == unloading || !entry.matches_module(&address.module) {
                    continue;
                }

                let hits = index.entry(address.address).or_default();
                let position = hits.partition_point(|hit| hit.key < *key);
                hits.insert(
                    position,
                    ApiHookHit {
                        key: key.clone(),
                        entry: entry.clone(),
                        module: Arc::from(address.module.as_str()),
                    },
                );
            }
        }

        let mut installed = Vec::new();
        for &address in index.keys() {
            if state.address_hooks.contains_key(&address)
                || state.retired_hooks.contains_key(&address)
            {
                continue;
            }

            let hook = context.hook_memory_execution_at(address, move |context, _| {
                if let Err(error) = context.api_hooks.dispatch(context, address) {
                    context.fail(error);
                }
            });
            match hook {
                Ok(hook) => installed.push((address, hook)),
                Err(error) => {
                    for (_, hook) in installed {
                        context.remove_hook(hook);
                    }
                    return Err(error);
                }
            }
        }

        state.index = index
            .into_iter()
            .map(|(address, hits)| (address, Arc::from(hits)))
            .collect();
        let stale = state
            .address_hooks
            .keys()
            .filter(|address| !state.index.contains_key(address))
            .copied()
            .collect::<Vec<_>>();
        for address in stale {
            state.release_address(context, address);
        }
        for address in state.index.keys() {
            if let Some(hook) = state.retired_hooks.remove(address) {
                state.address_hooks.insert(*address, hook);
            }
        }
        state.address_hooks.extend(installed);
        if let Some((key, entry)) = added {
            state.entries.insert(key, entry);
        }

        Ok(())
    }

    fn dispatch(&self, context: &Context, address: u64) -> Result<(), Error> {
        let Some(hits) = self.state.borrow().index.get(&address).cloned() else {
            return Ok(());
        };

        let mode = context.native.code_mode()?;
        let Some(return_address) = api_return_address(context, mode)? else {
            return Ok(());
        };

        let _guard = DispatchGuard::enter(self, context);
        let mut arguments = mem::take(&mut self.state.borrow_mut().arguments);
        for hit in hits.iter() {
            if !self.state.borrow().is_registered(hit) {
                continue;
            }

            let signature = hit.entry.signature;
            let stack_pointer = context.native.stack_pointer()?;
            let convention = match mode {
                CodeMode::X86 => signature.convention,
                _ => CallingConvention::Fastcall,
            };
            arguments.clear();
            for index in 0..signature.parameter_count as u64 {
                arguments.push(context.native.function_argument(convention, index)?);
            }

            let mut call = ApiCall {
                module: hit.module.clone(),
                name: hit.entry.name.clone(),
                address,
                return_address,
                return_value: 0,
            };
            if (hit.entry.callback)(context, &mut call, &arguments) == CallContinuation::Intercept {
                return_from_api(context, mode, &call, signature, stack_pointer)?;
                break;
            }
        }
        self.state.borrow_mut().arguments = arguments;

        Ok(())
    }
}

struct DispatchGuard<'a> {
    hooks: &'a ApiHooks,
    context: &'a Context,
}

impl<'a> DispatchGuard<'a> {
    fn enter(hooks: &'a ApiHooks, context: &'a Context) -> Self {
        hooks.state.borrow_mut().dispatch_depth += 1;
        Self { hooks, context }
    }
}

impl Drop for DispatchGuard<'_> {
    fn drop(&mut self) {
        let retired = {
            let mut state = self.hooks.state.borrow_mut();
            state.dispatch_depth -= 1;
            if state.dispatch_depth != 0 {
                return;
            }

            mem::take(&mut state.retired_hooks)
        };

        for hook in retired.into_values() {
            self.context.remove_hook(hook);
        }
    }
}

fn api_return_address(context: &Context, mode: CodeMode) -> Result<Option<u64>, Error> {
    if mode == CodeMode::Arm64 {
        return context.read_register(Arm64Register::X30).map(Some);
    }

    let stack_pointer = context.native.stack_pointer()?;
    let mut value = [0; 8];
    let size = if mode == CodeMode::X86 { 4 } else { 8 };
    Ok(context
        .try_read_memory(stack_pointer, &mut value[..size])
        .then(|| u64::from_le_bytes(value)))
}

/// Returns to the caller in place of the API: the callee's return, including a stdcall callee's argument pop.
fn return_from_api(
    context: &Context,
    mode: CodeMode,
    call: &ApiCall,
    signature: ApiSignature,
    stack_pointer: u64,
) -> Result<(), Error> {
    context.native.set_return_value(call.return_value)?;
    let stack_adjustment = match mode {
        CodeMode::Arm64 => None,
        CodeMode::X86 if signature.convention == CallingConvention::Stdcall => {
            Some(4 + 4 * signature.parameter_count as u64)
        }
        CodeMode::X86 => Some(4),
        _ => Some(8),
    };
    if let Some(adjustment) = stack_adjustment {
        context
            .native
            .set_stack_pointer(stack_pointer.wrapping_add(adjustment))?;
    }

    Ok(context
        .native
        .set_instruction_pointer(call.return_address)?)
}
