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
use std::ffi::c_void;

use melior::ExecutionEngine;

use crate::env::Env;

/// Function type for the main entrypoint of the generated code
pub type MainFunc = extern "C" fn(&mut SyscallContext, initial_gas: u64) -> u8;

#[derive(Debug, Clone)]
pub enum ExitStatusCode {
    Return = 0,
    Stop,
    Revert,
    Error,
    Default,
}
impl ExitStatusCode {
    #[inline(always)]
    pub fn to_u8(self) -> u8 {
        self as u8
    }
    pub fn from_u8(value: u8) -> Self {
        match value {
            x if x == Self::Return.to_u8() => Self::Return,
            x if x == Self::Stop.to_u8() => Self::Stop,
            x if x == Self::Revert.to_u8() => Self::Revert,
            x if x == Self::Error.to_u8() => Self::Error,
            _ => Self::Default,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ExecutionResult {
    Success {
        return_data: Vec<u8>,
        gas_remaining: u64,
    },
    Revert {
        return_data: Vec<u8>,
        gas_remaining: u64,
    },
    Halt,
}

impl ExecutionResult {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    pub fn is_revert(&self) -> bool {
        matches!(self, Self::Revert { .. })
    }

    pub fn is_halt(&self) -> bool {
        matches!(self, Self::Halt { .. })
    }

    pub fn return_data(&self) -> Option<&[u8]> {
        match self {
            Self::Success { return_data, .. } | Self::Revert { return_data, .. } => {
                Some(return_data)
            }
            Self::Halt => None,
        }
    }
}

/// The context passed to syscalls
#[derive(Debug, Default)]
pub struct SyscallContext {
    /// The memory segment of the EVM.
    /// For extending it, see [`Self::extend_memory`]
    memory: Vec<u8>,
    /// The result of the execution
    return_data: Option<(usize, usize)>,
    gas_remaining: Option<u64>,
    exit_status: Option<ExitStatusCode>,
    /// The execution environment. It contains chain, block, and tx data.
    #[allow(unused)]
    pub env: Env,
}

/// Accessors for disponibilizing the execution results
impl SyscallContext {
    pub fn with_env(env: Env) -> Self {
        Self {
            env,
            ..Self::default()
        }
    }
    pub fn return_values(&self) -> &[u8] {
        // TODO: maybe initialize as (0, 0) instead of None
        let (offset, size) = self.return_data.unwrap_or((0, 0));
        &self.memory[offset..offset + size]
    }

    pub fn get_result(&self) -> ExecutionResult {
        let gas_remaining = self.gas_remaining.unwrap_or(0);
        let exit_status = self.exit_status.clone().unwrap_or(ExitStatusCode::Default);
        match exit_status {
            ExitStatusCode::Return | ExitStatusCode::Stop => ExecutionResult::Success {
                return_data: self.return_values().to_vec(),
                gas_remaining,
            },
            ExitStatusCode::Revert => ExecutionResult::Revert {
                return_data: self.return_values().to_vec(),
                gas_remaining,
            },
            ExitStatusCode::Error | ExitStatusCode::Default => ExecutionResult::Halt,
        }
    }
}

/// Syscall implementations
///
/// Note that each function is marked as `extern "C"`, which is necessary for the
/// function to be callable from the generated code.
impl SyscallContext {
    pub extern "C" fn write_result(
        &mut self,
        offset: u32,
        bytes_len: u32,
        remaining_gas: u64,
        execution_result: u8,
    ) {
        self.return_data = Some((offset as usize, bytes_len as usize));
        self.gas_remaining = Some(remaining_gas);
        self.exit_status = Some(ExitStatusCode::from_u8(execution_result));
    }

    pub extern "C" fn get_callvalue(&self) -> u64 {
        let callvalue = self.env.tx.value;
        println!("Callvalue: {}", callvalue);
        callvalue
    }

    pub extern "C" fn get_calldata_size(&self) -> u32 {
        let size = self.env.tx.calldata.len();
        print!("Calldata size: {}", size as u32);
        self.env.tx.calldata.len() as u32
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
    pub const WRITE_RESULT: &str = "evm_mlir__write_result";
    pub const EXTEND_MEMORY: &str = "evm_mlir__extend_memory";
    pub const GET_CALLDATA_SIZE: &str = "evm_mlir__get_calldata_size";
    pub const GET_CALLVALUE: &str = "evm_mlir__get_callvalue";
}

/// Registers all the syscalls as symbols in the execution engine
///
/// This allows the generated code to call the syscalls by name.
pub fn register_syscalls(engine: &ExecutionEngine) {
    unsafe {
        engine.register_symbol(
            symbols::WRITE_RESULT,
            SyscallContext::write_result as *const fn(*mut c_void, u32, u32, u64, u8) as *mut (),
        );
        engine.register_symbol(
            symbols::EXTEND_MEMORY,
            SyscallContext::extend_memory as *const fn(*mut c_void, u32) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_CALLDATA_SIZE,
            SyscallContext::get_calldata_size as *const fn(*mut c_void) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_CALLVALUE,
            SyscallContext::get_callvalue as *const fn(*mut c_void) as *mut (),
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

    use super::symbols;

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
            TypeAttribute::new(
                FunctionType::new(context, &[ptr_type, uint32, uint32, uint64, uint8], &[]).into(),
            ),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::GET_CALLDATA_SIZE),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type], &[uint32]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::GET_CALLVALUE),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type], &[uint64]).into()),
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_result_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &Block,
        offset: Value,
        size: Value,
        gas: Value,
        reason: Value,
        location: Location,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::WRITE_RESULT),
            &[syscall_ctx, offset, size, gas, reason],
            &[],
            location,
        ));
    }

    pub(crate) fn get_calldata_size_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let uint32 = IntegerType::new(mlir_ctx, 32).into();
        let value = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_CALLDATA_SIZE),
                &[syscall_ctx],
                &[uint32],
                location,
            ))
            .result(0)?;
        Ok(value.into())
    }

    pub(crate) fn get_callvalue_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let uint64 = IntegerType::new(mlir_ctx, 64).into();
        let value = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_CALLVALUE),
                &[syscall_ctx],
                &[uint64],
                location,
            ))
            .result(0)?;
        Ok(value.into())
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
