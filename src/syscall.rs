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

use crate::{db::Db, env::Env};

/// Function type for the main entrypoint of the generated code
pub type MainFunc = extern "C" fn(&mut SyscallContext, initial_gas: u64) -> u8;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(C, align(16))]
pub struct U256 {
    pub lo: u128,
    pub hi: u128,
}

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
        logs: Vec<Log>,
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

    pub fn return_logs(&self) -> Option<&Vec<Log>> {
        match self {
            Self::Success { logs, .. } => Some(logs),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct InnerContext {
    /// The memory segment of the EVM.
    /// For extending it, see [`Self::extend_memory`]
    memory: Vec<u8>,
    /// The result of the execution
    return_data: Option<(usize, usize)>,
    gas_remaining: Option<u64>,
    exit_status: Option<ExitStatusCode>,
    logs: Vec<Log>,
}

/// The context passed to syscalls
#[derive(Debug)]
pub struct SyscallContext<'c> {
    pub env: Env,
    // The program bytecode
    pub program: Vec<u8>,
    pub db: &'c mut Db,
    pub inner_context: InnerContext,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Log {
    pub topics: Vec<U256>,
    pub data: Vec<u8>,
}

/// Accessors for disponibilizing the execution results
impl<'c> SyscallContext<'c> {
    pub fn new(env: Env, db: &'c mut Db) -> Self {
        Self {
            env,
            db,
            inner_context: Default::default(),
            program: Default::default(),
        }
    }

    pub fn return_values(&self) -> &[u8] {
        // TODO: maybe initialize as (0, 0) instead of None
        let (offset, size) = self.inner_context.return_data.unwrap_or((0, 0));
        &self.inner_context.memory[offset..offset + size]
    }

    pub fn get_result(&self) -> ExecutionResult {
        let gas_remaining = self.inner_context.gas_remaining.unwrap_or(0);
        let exit_status = self
            .inner_context
            .exit_status
            .clone()
            .unwrap_or(ExitStatusCode::Default);
        match exit_status {
            ExitStatusCode::Return | ExitStatusCode::Stop => ExecutionResult::Success {
                return_data: self.return_values().to_vec(),
                gas_remaining,
                logs: self.inner_context.logs.to_owned(),
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
impl<'c> SyscallContext<'c> {
    pub extern "C" fn write_result(
        &mut self,
        offset: u32,
        bytes_len: u32,
        remaining_gas: u64,
        execution_result: u8,
    ) {
        self.inner_context.return_data = Some((offset as usize, bytes_len as usize));
        self.inner_context.gas_remaining = Some(remaining_gas);
        self.inner_context.exit_status = Some(ExitStatusCode::from_u8(execution_result));
    }

    pub extern "C" fn get_calldata_size(&self) -> u32 {
        self.env.tx.calldata.len() as u32
    }

    pub extern "C" fn extend_memory(&mut self, new_size: u32) -> *mut u8 {
        let new_size = new_size as usize;
        if new_size <= self.inner_context.memory.len() {
            return self.inner_context.memory.as_mut_ptr();
        }
        match self
            .inner_context
            .memory
            .try_reserve(new_size - self.inner_context.memory.len())
        {
            Ok(()) => {
                self.inner_context.memory.resize(new_size, 0);
                self.inner_context.memory.as_mut_ptr()
            }
            // TODO: use tracing here
            Err(err) => {
                eprintln!("Failed to reserve memory: {err}");
                std::ptr::null_mut()
            }
        }
    }

    pub extern "C" fn copy_code_to_memory(
        &mut self,
        code_offset: u32,
        size: u32,
        dest_offset: u32,
    ) {
        let code_size = self.program.len() as u32;
        // if the offset is out of bounds then nothing is copied
        if code_offset >= code_size {
            return;
        }
        // adjust the size so it does not go out of bounds
        let size: u32 = if code_offset + size > code_size {
            code_size - code_offset
        } else {
            size
        };
        // copy the program into memory
        for (i, j) in (code_offset..code_offset + size).enumerate() {
            self.inner_context.memory[dest_offset as usize + i] = self.program[j as usize];
        }
    }
    pub extern "C" fn append_log(&mut self, offset: u32, size: u32) {
        self.create_log(offset, size, vec![]);
    }
    #[allow(improper_ctypes)]
    pub extern "C" fn append_log_with_one_topic(&mut self, offset: u32, size: u32, topic: &U256) {
        self.create_log(offset, size, vec![*topic]);
    }

    #[allow(improper_ctypes)]
    pub extern "C" fn append_log_with_two_topics(
        &mut self,
        offset: u32,
        size: u32,
        topic1: &U256,
        topic2: &U256,
    ) {
        self.create_log(offset, size, vec![*topic1, *topic2]);
    }

    #[allow(improper_ctypes)]
    pub extern "C" fn append_log_with_three_topics(
        &mut self,
        offset: u32,
        size: u32,
        topic1: &U256,
        topic2: &U256,
        topic3: &U256,
    ) {
        self.create_log(offset, size, vec![*topic1, *topic2, *topic3]);
    }

    #[allow(improper_ctypes)]
    pub extern "C" fn append_log_with_four_topics(
        &mut self,
        offset: u32,
        size: u32,
        topic1: &U256,
        topic2: &U256,
        topic3: &U256,
        topic4: &U256,
    ) {
        self.create_log(offset, size, vec![*topic1, *topic2, *topic3, *topic4]);
    }

    /// Receives a memory offset and size, and a vector of topics.
    /// Creates a Log with topics and data equal to memory[offset..offset + size]
    /// and pushes it to the logs vector.
    fn create_log(&mut self, offset: u32, size: u32, topics: Vec<U256>) {
        let offset = offset as usize;
        let size = size as usize;
        let data: Vec<u8> = self.inner_context.memory[offset..offset + size].into();

        let log = Log { data, topics };
        self.inner_context.logs.push(log);
    }
    pub extern "C" fn get_calldata_ptr(&mut self) -> *const u8 {
        self.env.tx.calldata.as_ptr()
    }
}

pub mod symbols {
    pub const WRITE_RESULT: &str = "evm_mlir__write_result";
    pub const EXTEND_MEMORY: &str = "evm_mlir__extend_memory";
    pub const APPEND_LOG: &str = "evm_mlir__append_log";
    pub const APPEND_LOG_ONE_TOPIC: &str = "evm_mlir__append_log_with_one_topic";
    pub const APPEND_LOG_TWO_TOPICS: &str = "evm_mlir__append_log_with_two_topics";
    pub const APPEND_LOG_THREE_TOPICS: &str = "evm_mlir__append_log_with_three_topics";
    pub const APPEND_LOG_FOUR_TOPICS: &str = "evm_mlir__append_log_with_four_topics";
    pub const GET_CALLDATA_PTR: &str = "evm_mlir__get_calldata_ptr";
    pub const GET_CALLDATA_SIZE: &str = "evm_mlir__get_calldata_size";
    pub const COPY_CODE_TO_MEMORY: &str = "evm_mlir__copy_code_to_memory";
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
            symbols::APPEND_LOG,
            SyscallContext::append_log as *const fn(*mut c_void, u32, u32) as *mut (),
        );
        engine.register_symbol(
            symbols::APPEND_LOG_ONE_TOPIC,
            SyscallContext::append_log_with_one_topic
                as *const fn(*mut c_void, u32, u32, *const U256) as *mut (),
        );
        engine.register_symbol(
            symbols::APPEND_LOG_TWO_TOPICS,
            SyscallContext::append_log_with_two_topics
                as *const fn(*mut c_void, u32, u32, *const U256, *const U256)
                as *mut (),
        );
        engine.register_symbol(
            symbols::APPEND_LOG_THREE_TOPICS,
            SyscallContext::append_log_with_three_topics
                as *const fn(*mut c_void, u32, u32, *const U256, *const U256, *const U256)
                as *mut (),
        );
        engine.register_symbol(
            symbols::APPEND_LOG_FOUR_TOPICS,
            SyscallContext::append_log_with_four_topics
                as *const fn(
                    *mut c_void,
                    u32,
                    u32,
                    *const U256,
                    *const U256,
                    *const U256,
                    *const U256,
                ) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_CALLDATA_PTR,
            SyscallContext::get_calldata_ptr as *const fn(*mut c_void) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_CALLDATA_SIZE,
            SyscallContext::get_calldata_size as *const fn(*mut c_void) as *mut (),
        );
        engine.register_symbol(
            symbols::COPY_CODE_TO_MEMORY,
            SyscallContext::copy_code_to_memory as *const fn(*mut c_void, u32, u32, u32) as *mut (),
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
        //let uint256 = IntegerType::new(context, 256).into();
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
            StringAttribute::new(context, symbols::EXTEND_MEMORY),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, uint32], &[ptr_type]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::COPY_CODE_TO_MEMORY),
            TypeAttribute::new(
                FunctionType::new(context, &[ptr_type, uint32, uint32, uint32], &[]).into(),
            ),
            Region::new(),
            attributes,
            location,
        ));
        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::APPEND_LOG),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, uint32, uint32], &[]).into()),
            Region::new(),
            attributes,
            location,
        ));
        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::APPEND_LOG_ONE_TOPIC),
            TypeAttribute::new(
                FunctionType::new(context, &[ptr_type, uint32, uint32, ptr_type], &[]).into(),
            ),
            Region::new(),
            attributes,
            location,
        ));
        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::APPEND_LOG_TWO_TOPICS),
            TypeAttribute::new(
                FunctionType::new(
                    context,
                    &[ptr_type, uint32, uint32, ptr_type, ptr_type],
                    &[],
                )
                .into(),
            ),
            Region::new(),
            attributes,
            location,
        ));
        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::APPEND_LOG_THREE_TOPICS),
            TypeAttribute::new(
                FunctionType::new(
                    context,
                    &[ptr_type, uint32, uint32, ptr_type, ptr_type, ptr_type],
                    &[],
                )
                .into(),
            ),
            Region::new(),
            attributes,
            location,
        ));
        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::APPEND_LOG_FOUR_TOPICS),
            TypeAttribute::new(
                FunctionType::new(
                    context,
                    &[
                        ptr_type, uint32, uint32, ptr_type, ptr_type, ptr_type, ptr_type,
                    ],
                    &[],
                )
                .into(),
            ),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::GET_CALLDATA_PTR),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type], &[ptr_type]).into()),
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

    /// Receives log data and appends a log to the logs vector
    pub(crate) fn append_log_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        data: Value<'c, 'c>,
        size: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::APPEND_LOG),
            &[syscall_ctx, data, size],
            &[],
            location,
        ));
    }

    /// Receives log data and a topic and appends a log to the logs vector
    pub(crate) fn append_log_with_one_topic_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        data: Value<'c, 'c>,
        size: Value<'c, 'c>,
        topic: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::APPEND_LOG_ONE_TOPIC),
            &[syscall_ctx, data, size, topic],
            &[],
            location,
        ));
    }

    /// Receives log data, two topics and appends a log to the logs vector
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn append_log_with_two_topics_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        data: Value<'c, 'c>,
        size: Value<'c, 'c>,
        topic1_ptr: Value<'c, 'c>,
        topic2_ptr: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::APPEND_LOG_TWO_TOPICS),
            &[syscall_ctx, data, size, topic1_ptr, topic2_ptr],
            &[],
            location,
        ));
    }

    /// Receives log data, three topics and appends a log to the logs vector
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn append_log_with_three_topics_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        data: Value<'c, 'c>,
        size: Value<'c, 'c>,
        topic1_ptr: Value<'c, 'c>,
        topic2_ptr: Value<'c, 'c>,
        topic3_ptr: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::APPEND_LOG_THREE_TOPICS),
            &[syscall_ctx, data, size, topic1_ptr, topic2_ptr, topic3_ptr],
            &[],
            location,
        ));
    }

    /// Receives log data, three topics and appends a log to the logs vector
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn append_log_with_four_topics_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        data: Value<'c, 'c>,
        size: Value<'c, 'c>,
        topic1_ptr: Value<'c, 'c>,
        topic2_ptr: Value<'c, 'c>,
        topic3_ptr: Value<'c, 'c>,
        topic4_ptr: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::APPEND_LOG_FOUR_TOPICS),
            &[
                syscall_ctx,
                data,
                size,
                topic1_ptr,
                topic2_ptr,
                topic3_ptr,
                topic4_ptr,
            ],
            &[],
            location,
        ));
    }
    /// Returns a pointer to the calldata.
    #[allow(unused)]
    pub(crate) fn get_calldata_ptr_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let ptr_type = pointer(mlir_ctx, 0);
        let value = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_CALLDATA_PTR),
                &[syscall_ctx],
                &[ptr_type],
                location,
            ))
            .result(0)?;
        Ok(value.into())
    }
    pub(crate) fn copy_code_to_memory_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        offset: Value,
        size: Value,
        dest_offset: Value,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::COPY_CODE_TO_MEMORY),
            &[syscall_ctx, offset, size, dest_offset],
            &[],
            location,
        ));
    }
}
