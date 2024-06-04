//! # Module implementing syscalls for the EVM
//!
//! The syscalls implemented here are to be exposed to the generated code
//! via [`register_syscalls`]. Each syscall implements functionality that's
//! not possible to implement in the generated code, such as interacting with
//! the storage, or just difficult, like allocating memory in the heap
//! ([`SyscallContext::extend_memory`]).
//!
//! ### Adding a new syscall
//!
//! New syscalls should be implemented by adding a new method to the [`SyscallContext`]
//! struct (see [`SyscallContext::write_result`] for an example). After that, the syscall
//! should be registered in the [`register_syscalls`] function, which will make it available
//! to the generated code. Afterwards, the syscall should be declared in
//! [`mlir::declare_syscalls`], which will make the syscall available inside the MLIR code.
//! Finally, the function can be called from the MLIR code like a normal function (see
//! [`mlir::write_result_syscall`] for an example).
use std::{default, ffi::c_void};

use melior::ExecutionEngine;

/// Function type for the main entrypoint of the generated code
pub type MainFunc = extern "C" fn(&mut SyscallContext);

#[derive(Debug,Default)]
pub enum ExecutionResult {
    #[default]
    Continue,
    Revert,
    Halt,
    Invalid,
}

impl ExecutionResult {
    fn to_u8(self) -> u8 {
        match self {
            ExecutionResult::Continue => 0,
            ExecutionResult::Revert => 1,
            ExecutionResult::Halt => 2,
            ExecutionResult::Invalid => 3,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            0 => ExecutionResult::Continue,
            1 => ExecutionResult::Revert,
            2 => ExecutionResult::Halt,
            _ => ExecutionResult::Invalid,
        }
    }
}


/// The context passed to syscalls
#[derive(Debug, Default)]
pub struct SyscallContext {
    /// The memory segment of the EVM.
    /// For extending it, see [`Self::extend_memory`]
    memory: Vec<u8>,
    /// The offset and size in [`Self::memory`] corresponding to the EVM return data.
    /// It's [`None`] in case there's no return data
    result: Option<(usize, usize)>,
    /// The gas available for the execution
    /// It's [`None`] in case the gas is not available
    gas_remaining: Option<u64>,
    /// Execution Result
    execution_result: ExecutionResult,
}

/// Accessors for disponibilizing the execution results
impl SyscallContext {
    pub fn return_values(&self) -> &[u8] {
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

    pub extern "C" fn store_revert_result(&mut self, offset: u32, bytes_len: u32, remaining_gas: u64, execution_result: u8) {
        self.result = Some((offset as usize, bytes_len as usize));
        self.gas_remaining = Some(remaining_gas);
        self.execution_result = ExecutionResult::from_u8(execution_result);
    }

    pub extern "C" fn extend_memory(&mut self, new_size: u32) -> *mut u8 {
        let new_size = new_size as usize;
        if new_size <= self.memory.len() {
            return self.memory.as_mut_ptr();
        }
        match self.memory.try_reserve(new_size - self.memory.len()) {
            Ok(()) => {
                self.memory.resize(new_size, 0);
                self.memory.as_mut_ptr()
            }
            // TODO: use tracing here
            Err(err) => {
                eprintln!("Failed to reserve memory: {err}");
                std::ptr::null_mut()
            }
        }
    }
}

pub mod symbols {
    pub const WRITE_RESULT: &str = "emv_mlir__write_result";
    pub const REVERT_RESULT: &str = "emv_mlir__store_revert_result";
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
            symbols::REVERT_RESULT,
            SyscallContext::store_revert_result as *const fn(*mut c_void, u32, u32, u64, u8) as *mut (),
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
            attribute::{FlatSymbolRefAttribute, StringAttribute, TypeAttribute},
            r#type::{FunctionType, IntegerType},
            Block, Identifier, Location, Module as MeliorModule, Region, Value,
        },
        Context as MeliorContext,
    };

    use crate::errors::CodegenError;

    use super::{symbols, ExecutionResult};

    pub(crate) fn declare_syscalls(context: &MeliorContext, module: &MeliorModule) {
        let location = Location::unknown(context);

        // Type declarations
        let ptr_type = pointer(context, 0);
        let uint32 = IntegerType::new(context, 32).into();
        let uint64 = IntegerType::new(context, 64).into();
        let uint8 = IntegerType::new(context, 8).into();

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
            StringAttribute::new(context, symbols::REVERT_RESULT),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, uint32, uint32, uint64,uint8], &[]).into()),
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

    /// Stores the return values in the syscall context
    pub(crate) fn write_result_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &Block,
        offset: Value,
        size: Value,
        location: Location,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::WRITE_RESULT),
            &[syscall_ctx, offset, size],
            &[],
            location,
        ));
    }

    /// Stores the revert values in the syscall context
    pub(crate) fn write_revert_result_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &Block,
        offset: Value,
        size: Value,
        remaining_gas: Value,
        execution_result:Value,
        location: Location,
    ) {
       block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::REVERT_RESULT),
            &[syscall_ctx, offset,size,remaining_gas,execution_result],
            &[],
            location,
        )); 
    }
    
    /// Extends the memory segment of the syscall context.
    /// Returns a pointer to the start of the memory segment.
    pub(crate) fn extend_memory_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        new_size: Value<'c, 'c>,
        location: Location<'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let ptr_type = pointer(mlir_ctx, 0);
        let value = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::EXTEND_MEMORY),
                &[syscall_ctx, new_size],
                &[ptr_type],
                location,
            ))
            .result(0)?;
        Ok(value.into())
    }
}
