//! # Module implementing syscalls for the EVM
//!
//! The syscalls implemented here are to be exposed to the generated code,
//! for example, via [`register_syscalls`]. Each syscall implements functionality
//! that's not possible to implement in the EVM itself, such as interacting with
//! the storage, or just difficult, like allocating memory in the heap
//! ([`SyscallContext::extend_memory`]).
//!
//! ### Adding a new syscall
//!
//! New syscalls should be implemented by adding a new method to the [`SyscallContext`]
//! struct (see [`SyscallContext::write_result`] for an example). After that, the syscall
//! should be registered in the [`register_syscalls`] function, which will make it available
//! to the generated code. Afterwards, the syscall should be declared in
//! [`mlir::declare_syscalls`]. This will make the syscall available inside the MLIR code.
use std::ffi::c_void;

use melior::ExecutionEngine;

/// Function type for the main entrypoint of the generated code
pub type MainFunc = extern "C" fn(&mut SyscallContext);

/// The context passed to syscalls
#[derive(Debug, Default)]
pub struct SyscallContext {
    /// The memory segment of the EVM.
    /// For extending it, see [`Self::extend_memory`]
    memory: Vec<u8>,
    /// The offset and size in [`Self::memory`] corresponding to the EVM return data.
    /// It's [`None`] in case there's no return data
    result: Option<(usize, usize)>,
}

/// Accessors for disponibilizing the execution results
impl SyscallContext {
    pub fn result(&self) -> &[u8] {
        // TODO: maybe initialize as (0, 0) instead of None
        let (offset, size) = self.result.unwrap_or((0, 0));
        &self.memory[offset..offset + size]
    }
}

/// Syscall implementations
///
/// Note that each function is marked as `extern "C"`, which is necessary for the
/// function to be callable from the generated code.
impl SyscallContext {
    pub extern "C" fn write_result(&mut self, offset: u32, bytes_len: u32) {
        self.result = Some((offset as usize, bytes_len as usize));
    }

    pub extern "C" fn extend_memory(&mut self, new_size: u32) -> *mut u8 {
        let new_size = new_size as usize;
        if new_size > self.memory.len() {
            // TODO: check for OOM
            self.memory.resize(new_size, 0);
        }
        self.memory.as_mut_ptr()
    }
}

pub mod symbols {
    pub const WRITE_RESULT: &str = "emv_mlir__write_result";
    pub const EXTEND_MEMORY: &str = "emv_mlir__extend_memory";
}

/// Registers all the syscalls as symbols in the execution engine
///
/// This allows the generated code to call the syscalls by name.
pub fn register_syscalls(engine: &ExecutionEngine) {
    unsafe {
        engine.register_symbol(
            symbols::WRITE_RESULT,
            SyscallContext::write_result as *const fn(*mut c_void, u32, u32) as *mut (),
        );
        engine.register_symbol(
            symbols::EXTEND_MEMORY,
            SyscallContext::extend_memory as *const fn(*mut c_void, u32) as *mut (),
        );
    };
}

/// MLIR util for declaring syscalls
pub(crate) mod mlir {
    use melior::{
        dialect::{func, llvm::r#type::pointer},
        ir::{
            attribute::{StringAttribute, TypeAttribute},
            r#type::{FunctionType, IntegerType},
            Identifier, Location, Module as MeliorModule, Region,
        },
        Context as MeliorContext,
    };

    use super::symbols;

    pub(crate) fn declare_syscalls(context: &MeliorContext, module: &MeliorModule) {
        let location = Location::unknown(context);

        // Type declarations
        let ptr_type = pointer(context, 0);
        let uint32 = IntegerType::new(context, 32).into();

        let attributes = &[(
            Identifier::new(context, "sym_visibility"),
            StringAttribute::new(context, "private").into(),
        )];

        // Syscall declarations
        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::WRITE_RESULT),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, uint32, uint32], &[]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::EXTEND_MEMORY),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, uint32], &[ptr_type]).into()),
            Region::new(),
            attributes,
            location,
        ));
    }
}
