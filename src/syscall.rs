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
use std::{ffi::c_void, path::PathBuf};

use crate::{
    context::Context,
    db::{AccountInfo, Database, Db},
    env::{Env, TransactTo},
    executor::{Executor, OptLevel},
    primitives::{Address, Bytes, U256 as EU256},
    program::Program,
    result::{EVMError, ExecutionResult, HaltReason, Output, ResultAndState, SuccessReason},
};
use melior::ExecutionEngine;
use sha3::{Digest, Keccak256};

/// Function type for the main entrypoint of the generated code
pub type MainFunc = extern "C" fn(&mut SyscallContext, initial_gas: u64) -> u8;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(C, align(16))]
pub struct U256 {
    pub lo: u128,
    pub hi: u128,
}

impl U256 {
    pub fn from_be_bytes(bytes: [u8; 32]) -> Self {
        let hi = u128::from_be_bytes(bytes[0..16].try_into().unwrap());
        let lo = u128::from_be_bytes(bytes[16..32].try_into().unwrap());
        U256 { hi, lo }
    }

    pub fn copy_from_address(&mut self, value: &Address) {
        let mut buffer = [0u8; 32];
        buffer[12..32].copy_from_slice(&value.0);
        self.lo = u128::from_be_bytes(buffer[16..32].try_into().unwrap());
        self.hi = u128::from_be_bytes(buffer[0..16].try_into().unwrap());
    }

    pub fn to_address(&self) -> Address {
        let hi_bytes = self.hi.to_be_bytes();
        let lo_bytes = self.lo.to_be_bytes();
        let address = [&hi_bytes[12..16], &lo_bytes[..]].concat();
        Address::from_slice(&address)
    }

    pub fn to_primitive_u256(&self) -> EU256 {
        let hi_bytes = self.hi.to_be_bytes();
        let lo_bytes = self.lo.to_be_bytes();
        let bytes = [&hi_bytes[..], &lo_bytes[..]].concat();
        EU256::from_big_endian(&bytes)
    }
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

#[derive(Debug, Default)]
pub struct InnerContext {
    /// The memory segment of the EVM.
    /// For extending it, see [`Self::extend_memory`]
    memory: Vec<u8>,
    /// The result of the execution
    return_data: Option<(usize, usize)>,
    // The program bytecode
    pub program: Vec<u8>,
    gas_remaining: Option<u64>,
    exit_status: Option<ExitStatusCode>,
    logs: Vec<LogData>,
}

/// The context passed to syscalls
#[derive(Debug)]
pub struct SyscallContext<'c> {
    pub env: Env,
    pub db: &'c mut Db,
    pub inner_context: InnerContext,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct LogData {
    pub topics: Vec<U256>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct Log {
    pub address: Address,
    pub data: LogData,
}

/// Accessors for disponibilizing the execution results
impl<'c> SyscallContext<'c> {
    pub fn new(env: Env, db: &'c mut Db) -> Self {
        Self {
            env,
            db,
            inner_context: Default::default(),
        }
    }

    pub fn return_values(&self) -> &[u8] {
        let (offset, size) = self.inner_context.return_data.unwrap_or((0, 0));
        &self.inner_context.memory[offset..offset + size]
    }

    pub fn logs(&self) -> Vec<Log> {
        self.inner_context
            .logs
            .iter()
            .map(|logdata| Log {
                address: self.env.tx.caller,
                data: logdata.clone(),
            })
            .collect()
    }

    pub fn get_result(&self) -> Result<ResultAndState, EVMError> {
        let gas_remaining = self.inner_context.gas_remaining.unwrap_or(0);
        let gas_initial = self.env.tx.gas_limit;
        let gas_used = gas_initial.saturating_sub(gas_remaining);
        let exit_status = self
            .inner_context
            .exit_status
            .clone()
            .unwrap_or(ExitStatusCode::Default);
        let return_values = self.return_values().to_vec();
        let result = match exit_status {
            ExitStatusCode::Return => ExecutionResult::Success {
                reason: SuccessReason::Return,
                gas_used,
                gas_refunded: 0, // TODO: implement gas refunds
                output: Output::Call(return_values.into()), // TODO: add case Output::Create
                logs: self.logs(),
            },
            ExitStatusCode::Stop => ExecutionResult::Success {
                reason: SuccessReason::Stop,
                gas_used,
                gas_refunded: 0, // TODO: implement gas refunds
                output: Output::Call(return_values.into()), // TODO: add case Output::Create
                logs: self.logs(),
            },
            ExitStatusCode::Revert => ExecutionResult::Revert {
                output: return_values.into(),
                gas_used,
            },
            ExitStatusCode::Error | ExitStatusCode::Default => ExecutionResult::Halt {
                reason: HaltReason::OpcodeNotFound, // TODO: check which Halt error
                gas_used,
            },
        };

        let state = self.db.clone().into_state();

        Ok(ResultAndState { result, state })
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

    pub extern "C" fn call(
        &mut self,
        mut gas_to_send: u64,
        call_to_address: &U256,
        value_to_transfer: &U256,
        args_offset: u32,
        args_size: u32,
        ret_offset: u32,
        ret_size: u32,
        available_gas: u64,
        consumed_gas: &mut u64,
    ) -> u32 {
        //TODO: Add call depth check
        //Return values
        const SUCCESS_RETURN_CODE: u32 = 1;
        const REVERT_RETURN_CODE: u32 = 0;
        const HALT_RETURN_CODE: u32 = 2;

        //Gas costs constants
        const WARM_MEMORY_ACCESS_COST: u64 = 100;
        const NOT_ZERO_VALUE_COST: u64 = 9000;
        const EMPTY_CALLE_COST: u64 = 25000;
        const STIPEND_GAS_ADDITION: u64 = 2300;
        const GAS_CAP_DIVISION_FACTOR: u64 = 64;
        //TODO: Check that the args offsets and sizes are correct -> This from the MLIR side
        let callee_address = call_to_address.to_address();
        let value = value_to_transfer.to_primitive_u256();

        //TODO: This should instead add the account fetch (warm or cold) cost
        //For the moment we consider warm access
        let callee_account = match self.db.basic(callee_address) {
            Ok(maybe_account) => {
                *consumed_gas = WARM_MEMORY_ACCESS_COST;
                maybe_account.unwrap_or_default()
            }
            Err(_) => {
                *consumed_gas = 0;
                return REVERT_RETURN_CODE;
            }
        };

        let caller_address = self.env.tx.caller;
        let caller_account = self
            .db
            .basic(caller_address)
            .unwrap() //We are sure it exists
            .unwrap_or_default();

        let mut stipend = 0;
        if !value.is_zero() {
            if caller_account.balance < value {
                //There isn't enough balance to send
                return REVERT_RETURN_CODE;
            }
            *consumed_gas += NOT_ZERO_VALUE_COST;
            if callee_account.is_empty() {
                *consumed_gas += EMPTY_CALLE_COST;
            }
            if available_gas < *consumed_gas {
                return REVERT_RETURN_CODE; //It acctually doesn't matter what we return here
            }
            stipend = STIPEND_GAS_ADDITION;

            //TODO: Maybe we should increment the nonce too
            let caller_balance = caller_account.balance;
            let caller_nonce = caller_account.nonce;
            self.db
                .update_account(caller_address, caller_nonce, caller_balance - value);

            let callee_balance = callee_account.balance;
            let callee_nonce = callee_account.nonce;
            self.db
                .update_account(callee_address, callee_nonce, callee_balance + value);
        }

        let remaining_gas = available_gas - *consumed_gas;
        gas_to_send = std::cmp::min(remaining_gas / GAS_CAP_DIVISION_FACTOR, gas_to_send);
        *consumed_gas += gas_to_send;
        gas_to_send += stipend;

        let mut env = self.env.clone();
        env.tx.transact_to = TransactTo::Call(callee_address);
        //TODO: Check if this is ok
        env.tx.caller = match self.env.tx.transact_to {
            TransactTo::Call(a) => a,
            TransactTo::Create => Address::zero(),
        };
        env.tx.value = value;
        env.tx.gas_limit = gas_to_send;

        //Copy the calldata from memory
        let off = args_offset as usize;
        let size = args_size as usize;
        env.tx.data = Bytes::from(self.inner_context.memory[off..off + size].to_vec());

        //NOTE: We could optimize this by not making the call if the bytecode is zero.
        //We would have to refund the stipend here
        let bytecode = self
            .db
            .code_by_address(callee_address)
            .expect("failed to load bytecode");
        let program = Program::from_bytecode(&bytecode).unwrap();

        //TODO: REMOVE THIS -> For the moment we don't need the output, so we just write to a
        //fixed output file.
        //In the future we will probably fetch the disk to search for the requested contract
        //bytecode. Then, if it is not present, we will compile it and store it in the disk
        let output_file = PathBuf::from("output2");

        let context = Context::new();
        let module = context
            .compile(&program, &output_file)
            .expect("failed to compile program");

        let executor = Executor::new(&module, OptLevel::Aggressive);
        let mut context = SyscallContext::new(env.clone(), self.db);

        executor.execute(&mut context, env.tx.gas_limit);

        match context.get_result().unwrap().result {
            ExecutionResult::Success {
                gas_used, output, ..
            } => {
                self.write_to_memory(ret_offset as _, ret_size as _, output.data());
                *consumed_gas -= gas_to_send - gas_used;
                SUCCESS_RETURN_CODE
            }
            //TODO: If we revert, should we still send the value to the called contract?
            ExecutionResult::Revert {
                gas_used, output, ..
            } => {
                self.write_to_memory(ret_offset as _, ret_size as _, &output);
                *consumed_gas -= gas_to_send - gas_used;
                REVERT_RETURN_CODE
            }
            ExecutionResult::Halt { gas_used, .. } => {
                *consumed_gas -= gas_to_send - gas_used;
                HALT_RETURN_CODE
            }
        }
    }

    fn write_to_memory(&mut self, offset: usize, size: usize, data: &[u8]) {
        self.inner_context.memory[offset..offset + size].copy_from_slice(data);
    }

    pub extern "C" fn store_in_selfbalance_ptr(&mut self, balance: &mut U256) {
        let account = match self.env.tx.transact_to {
            TransactTo::Call(address) => self.db.basic(address).unwrap().unwrap_or_default(),
            TransactTo::Create => AccountInfo::default(), //This branch should never happen
        };
        balance.hi = (account.balance >> 128).low_u128();
        balance.lo = account.balance.low_u128();
    }

    pub extern "C" fn keccak256_hasher(&mut self, offset: u32, size: u32, hash_ptr: &mut U256) {
        let offset = offset as usize;
        let size = size as usize;
        let data = &self.inner_context.memory[offset..offset + size];
        let mut hasher = Keccak256::new();
        hasher.update(data);
        let result = hasher.finalize();
        *hash_ptr = U256::from_be_bytes(result.into());
    }

    pub extern "C" fn store_in_callvalue_ptr(&self, value: &mut U256) {
        let aux = &self.env.tx.value;
        value.lo = aux.low_u128();
        value.hi = (aux >> 128).low_u128();
    }

    pub extern "C" fn store_in_blobbasefee_ptr(&self, value: &mut U256) {
        let aux = &self.env.block.blob_base_fee;
        value.lo = aux.low_u128();
        value.hi = (aux >> 128).low_u128();
    }

    pub extern "C" fn store_in_caller_ptr(&self, value: &mut U256) {
        //TODO: Here we are returning the tx.caller value, which in fact corresponds to ORIGIN
        //opcode. For the moment it's ok, but it should be changed when we implement the CALL opcode.
        let bytes = &self.env.tx.caller.to_fixed_bytes();
        let high: [u8; 16] = [&[0u8; 12], &bytes[..4]].concat().try_into().unwrap();
        let low: [u8; 16] = bytes[4..20].try_into().unwrap();
        //Now, we have to swap endianess, since data will be interpreted as it comes from
        //little endiann, aligned to 16 bytes
        value.lo = u128::from_be_bytes(low);
        value.hi = u128::from_be_bytes(high);
    }

    pub extern "C" fn store_in_gasprice_ptr(&self, value: &mut U256) {
        let aux = &self.env.tx.gas_price;
        value.lo = aux.low_u128();
        value.hi = (aux >> 128).low_u128();
    }

    pub extern "C" fn get_chainid(&self) -> u64 {
        self.env.cfg.chain_id
    }

    pub extern "C" fn get_calldata_ptr(&mut self) -> *const u8 {
        self.env.tx.data.as_ptr()
    }

    pub extern "C" fn get_calldata_size_syscall(&self) -> u32 {
        self.env.tx.data.len() as u32
    }

    pub extern "C" fn get_origin(&self, address: &mut U256) {
        let aux = &self.env.tx.caller;
        address.copy_from_address(aux);
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
        let code_size = self.inner_context.program.len();
        // cast everything to `usize`
        let code_offset = code_offset as usize;
        let size = size as usize;
        let dest_offset = dest_offset as usize;

        // adjust the size so it does not go out of bounds
        let size: usize = if code_offset + size > code_size {
            code_size.saturating_sub(code_offset)
        } else {
            size
        };

        let code_slice = &self.inner_context.program[code_offset..code_offset + size];
        // copy the program into memory
        self.inner_context.memory[dest_offset..dest_offset + size].copy_from_slice(code_slice);
    }

    pub extern "C" fn read_storage(&mut self, stg_key: &U256, stg_value: &mut U256) {
        let address = self.env.tx.caller;

        let key = ((EU256::from(stg_key.hi)) << 128) + stg_key.lo;

        let result = self.db.read_storage(address, key);

        stg_value.hi = (result >> 128).low_u128();
        stg_value.lo = result.low_u128();
    }

    pub extern "C" fn append_log(&mut self, offset: u32, size: u32) {
        self.create_log(offset, size, vec![]);
    }

    pub extern "C" fn append_log_with_one_topic(&mut self, offset: u32, size: u32, topic: &U256) {
        self.create_log(offset, size, vec![*topic]);
    }

    pub extern "C" fn append_log_with_two_topics(
        &mut self,
        offset: u32,
        size: u32,
        topic1: &U256,
        topic2: &U256,
    ) {
        self.create_log(offset, size, vec![*topic1, *topic2]);
    }

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

    pub extern "C" fn get_block_number(&self, number: &mut U256) {
        let block_number = self.env.block.number;

        number.hi = (block_number >> 128).low_u128();
        number.lo = block_number.low_u128();
    }

    /// Receives a memory offset and size, and a vector of topics.
    /// Creates a Log with topics and data equal to memory[offset..offset + size]
    /// and pushes it to the logs vector.
    fn create_log(&mut self, offset: u32, size: u32, topics: Vec<U256>) {
        let offset = offset as usize;
        let size = size as usize;
        let data: Vec<u8> = self.inner_context.memory[offset..offset + size].into();

        let log = LogData { data, topics };
        self.inner_context.logs.push(log);
    }

    pub extern "C" fn get_address_ptr(&mut self) -> *const u8 {
        self.env.tx.get_address().to_fixed_bytes().as_ptr()
    }

    pub extern "C" fn get_coinbase_ptr(&self) -> *const u8 {
        self.env.block.coinbase.as_ptr()
    }

    pub extern "C" fn store_in_timestamp_ptr(&self, value: &mut U256) {
        let aux = &self.env.block.timestamp;
        value.lo = aux.low_u128();
        value.hi = (aux >> 128).low_u128();
    }

    pub extern "C" fn store_in_basefee_ptr(&self, basefee: &mut U256) {
        basefee.hi = (self.env.block.basefee >> 128).low_u128();
        basefee.lo = self.env.block.basefee.low_u128();
    }

    pub extern "C" fn store_in_balance(&mut self, address: &U256, balance: &mut U256) {
        // addresses longer than 20 bytes should be invalid
        if (address.hi >> 32) != 0 {
            balance.hi = 0;
            balance.lo = 0;
        } else {
            let address_hi_slice = address.hi.to_be_bytes();
            let address_lo_slice = address.lo.to_be_bytes();

            let address_slice = [&address_hi_slice[12..16], &address_lo_slice[..]].concat();

            let address = Address::from_slice(&address_slice);

            match self.db.basic(address).unwrap() {
                Some(a) => {
                    balance.hi = (a.balance >> 128).low_u128();
                    balance.lo = a.balance.low_u128();
                }
                None => {
                    balance.hi = 0;
                    balance.lo = 0;
                }
            };
        }
    }
}

pub mod symbols {
    pub const WRITE_RESULT: &str = "evm_mlir__write_result";
    pub const EXTEND_MEMORY: &str = "evm_mlir__extend_memory";
    pub const KECCAK256_HASHER: &str = "evm_mlir__keccak256_hasher";
    pub const STORAGE_READ: &str = "evm_mlir__read_storage";
    pub const APPEND_LOG: &str = "evm_mlir__append_log";
    pub const APPEND_LOG_ONE_TOPIC: &str = "evm_mlir__append_log_with_one_topic";
    pub const APPEND_LOG_TWO_TOPICS: &str = "evm_mlir__append_log_with_two_topics";
    pub const APPEND_LOG_THREE_TOPICS: &str = "evm_mlir__append_log_with_three_topics";
    pub const APPEND_LOG_FOUR_TOPICS: &str = "evm_mlir__append_log_with_four_topics";
    pub const GET_CALLDATA_PTR: &str = "evm_mlir__get_calldata_ptr";
    pub const GET_CALLDATA_SIZE: &str = "evm_mlir__get_calldata_size";
    pub const COPY_CODE_TO_MEMORY: &str = "evm_mlir__copy_code_to_memory";
    pub const GET_ADDRESS_PTR: &str = "evm_mlir__get_address_ptr";
    pub const STORE_IN_CALLVALUE_PTR: &str = "evm_mlir__store_in_callvalue_ptr";
    pub const STORE_IN_BLOBBASEFEE_PTR: &str = "evm_mlir__store_in_blobbasefee_ptr";
    pub const STORE_IN_BALANCE: &str = "evm_mlir__store_in_balance";
    pub const GET_COINBASE_PTR: &str = "evm_mlir__get_coinbase_ptr";
    pub const STORE_IN_TIMESTAMP_PTR: &str = "evm_mlir__store_in_timestamp_ptr";
    pub const STORE_IN_BASEFEE_PTR: &str = "evm_mlir__store_in_basefee_ptr";
    pub const STORE_IN_CALLER_PTR: &str = "evm_mlir__store_in_caller_ptr";
    pub const GET_ORIGIN: &str = "evm_mlir__get_origin";
    pub const GET_CHAINID: &str = "evm_mlir__get_chainid";
    pub const STORE_IN_GASPRICE_PTR: &str = "evm_mlir__store_in_gasprice_ptr";
    pub const GET_BLOCK_NUMBER: &str = "evm_mlir__get_block_number";
    pub const STORE_IN_SELFBALANCE_PTR: &str = "evm_mlir__store_in_selfbalance_ptr";
    pub const CALL: &str = "evm_mlir__call";
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
            symbols::KECCAK256_HASHER,
            SyscallContext::keccak256_hasher as *const fn(*mut c_void, u32, u32, *const U256)
                as *mut (),
        );
        engine.register_symbol(
            symbols::EXTEND_MEMORY,
            SyscallContext::extend_memory as *const fn(*mut c_void, u32) as *mut (),
        );
        engine.register_symbol(
            symbols::STORAGE_READ,
            SyscallContext::read_storage as *const fn(*const c_void, *const U256, *mut U256)
                as *mut (),
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
            symbols::CALL,
            SyscallContext::call
                as *const fn(
                    *mut c_void,
                    u64,
                    *const U256,
                    *const U256,
                    u32,
                    u32,
                    u32,
                    u32,
                    u64,
                    *mut u64,
                ) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_CALLDATA_PTR,
            SyscallContext::get_calldata_ptr as *const fn(*mut c_void) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_CALLDATA_SIZE,
            SyscallContext::get_calldata_size_syscall as *const fn(*mut c_void) as *mut (),
        );
        engine.register_symbol(
            symbols::EXTEND_MEMORY,
            SyscallContext::extend_memory as *const fn(*mut c_void, u32) as *mut (),
        );
        engine.register_symbol(
            symbols::COPY_CODE_TO_MEMORY,
            SyscallContext::copy_code_to_memory as *const fn(*mut c_void, u32, u32, u32) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_ORIGIN,
            SyscallContext::get_origin as *const fn(*mut c_void, *mut U256) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_ADDRESS_PTR,
            SyscallContext::get_address_ptr as *const fn(*mut c_void) as *mut (),
        );
        engine.register_symbol(
            symbols::STORE_IN_CALLVALUE_PTR,
            SyscallContext::store_in_callvalue_ptr as *const fn(*mut c_void, *mut U256) as *mut (),
        );
        engine.register_symbol(
            symbols::STORE_IN_BLOBBASEFEE_PTR,
            SyscallContext::store_in_blobbasefee_ptr
                as *const extern "C" fn(&SyscallContext, *mut U256) -> () as *mut (),
        );
        engine.register_symbol(
            symbols::GET_COINBASE_PTR,
            SyscallContext::get_coinbase_ptr as *const fn(*mut c_void) as *mut (),
        );
        engine.register_symbol(
            symbols::STORE_IN_TIMESTAMP_PTR,
            SyscallContext::store_in_timestamp_ptr as *const fn(*mut c_void, *mut U256) as *mut (),
        );
        engine.register_symbol(
            symbols::STORE_IN_BASEFEE_PTR,
            SyscallContext::store_in_basefee_ptr as *const fn(*mut c_void, *mut U256) as *mut (),
        );
        engine.register_symbol(
            symbols::STORE_IN_CALLER_PTR,
            SyscallContext::store_in_caller_ptr as *const fn(*mut c_void, *mut U256) as *mut (),
        );
        engine.register_symbol(
            symbols::STORE_IN_GASPRICE_PTR,
            SyscallContext::store_in_gasprice_ptr as *const fn(*mut c_void, *mut U256) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_BLOCK_NUMBER,
            SyscallContext::get_block_number as *const fn(*mut c_void, *mut U256) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_CHAINID,
            SyscallContext::get_chainid as *const extern "C" fn(&SyscallContext) -> u64 as *mut (),
        );
        engine.register_symbol(
            symbols::STORE_IN_BALANCE,
            SyscallContext::store_in_balance as *const fn(*mut c_void, *const U256, *mut U256)
                as *mut (),
        );
        engine.register_symbol(
            symbols::STORE_IN_SELFBALANCE_PTR,
            SyscallContext::store_in_selfbalance_ptr as *const extern "C" fn(&SyscallContext) -> u64
                as *mut (),
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
            StringAttribute::new(context, symbols::KECCAK256_HASHER),
            TypeAttribute::new(
                FunctionType::new(context, &[ptr_type, uint32, uint32, ptr_type], &[]).into(),
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
            StringAttribute::new(context, symbols::GET_CHAINID),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type], &[uint64]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::STORE_IN_CALLVALUE_PTR),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
            Region::new(),
            attributes,
            location,
        ));
        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::STORE_IN_CALLER_PTR),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::STORE_IN_GASPRICE_PTR),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::STORE_IN_SELFBALANCE_PTR),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::STORE_IN_BLOBBASEFEE_PTR),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
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
            StringAttribute::new(context, symbols::STORAGE_READ),
            r#TypeAttribute::new(
                FunctionType::new(context, &[ptr_type, ptr_type, ptr_type], &[]).into(),
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
            StringAttribute::new(context, symbols::GET_ORIGIN),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::GET_COINBASE_PTR),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type], &[ptr_type]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::GET_BLOCK_NUMBER),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::GET_ADDRESS_PTR),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type], &[ptr_type]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::STORE_IN_TIMESTAMP_PTR),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::STORE_IN_BASEFEE_PTR),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::CALL),
            TypeAttribute::new(
                FunctionType::new(
                    context,
                    &[
                        ptr_type, uint64, ptr_type, ptr_type, uint32, uint32, uint32, uint32,
                        uint64, ptr_type,
                    ],
                    &[uint32],
                )
                .into(),
            ),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::STORE_IN_BALANCE),
            TypeAttribute::new(
                FunctionType::new(context, &[ptr_type, ptr_type, ptr_type], &[]).into(),
            ),
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

    pub(crate) fn keccak256_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        offset: Value<'c, 'c>,
        size: Value<'c, 'c>,
        hash_ptr: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::KECCAK256_HASHER),
            &[syscall_ctx, offset, size, hash_ptr],
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

    /// Returns a pointer to the start of the calldata
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

    pub(crate) fn get_chainid_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let uint64 = IntegerType::new(mlir_ctx, 64).into();
        let value = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_CHAINID),
                &[syscall_ctx],
                &[uint64],
                location,
            ))
            .result(0)?;
        Ok(value.into())
    }

    pub(crate) fn store_in_callvalue_ptr<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
        callvalue_ptr: Value<'c, 'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::STORE_IN_CALLVALUE_PTR),
            &[syscall_ctx, callvalue_ptr],
            &[],
            location,
        ));
    }

    pub(crate) fn store_in_blobbasefee_ptr<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
        blob_base_fee_ptr: Value<'c, 'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::STORE_IN_BLOBBASEFEE_PTR),
            &[syscall_ctx, blob_base_fee_ptr],
            &[],
            location,
        ));
    }

    pub(crate) fn store_in_gasprice_ptr<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
        gasprice_ptr: Value<'c, 'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::STORE_IN_GASPRICE_PTR),
            &[syscall_ctx, gasprice_ptr],
            &[],
            location,
        ));
    }

    pub(crate) fn store_in_caller_ptr<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
        caller_ptr: Value<'c, 'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::STORE_IN_CALLER_PTR),
            &[syscall_ctx, caller_ptr],
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

    pub(crate) fn store_in_selfbalance_ptr<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
        balance_ptr: Value<'c, 'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::STORE_IN_SELFBALANCE_PTR),
            &[syscall_ctx, balance_ptr],
            &[],
            location,
        ));
    }

    /// Reads the storage given a key
    pub(crate) fn storage_read_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        key: Value<'c, 'c>,
        value: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::STORAGE_READ),
            &[syscall_ctx, key, value],
            &[],
            location,
        ));
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

    pub(crate) fn get_origin_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        address_pointer: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_ORIGIN),
            &[syscall_ctx, address_pointer],
            &[],
            location,
        ));
    }

    /// Returns a pointer to the coinbase address.
    pub(crate) fn get_coinbase_ptr_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let ptr_type = pointer(mlir_ctx, 0);
        let value = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_COINBASE_PTR),
                &[syscall_ctx],
                &[ptr_type],
                location,
            ))
            .result(0)?;
        Ok(value.into())
    }

    /// Returns the block number.
    #[allow(unused)]
    pub(crate) fn get_block_number_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        number: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_BLOCK_NUMBER),
            &[syscall_ctx, number],
            &[],
            location,
        ));
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

    /// Returns a pointer to the address of the current executing contract
    #[allow(unused)]
    pub(crate) fn get_address_ptr_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let uint256 = IntegerType::new(mlir_ctx, 256);
        let ptr_type = pointer(mlir_ctx, 0);
        let value = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_ADDRESS_PTR),
                &[syscall_ctx],
                &[ptr_type],
                location,
            ))
            .result(0)?;
        Ok(value.into())
    }

    /// Stores the current block's timestamp in the `timestamp_ptr`.
    pub(crate) fn store_in_timestamp_ptr<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
        timestamp_ptr: Value<'c, 'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::STORE_IN_TIMESTAMP_PTR),
            &[syscall_ctx, timestamp_ptr],
            &[],
            location,
        ));
    }

    #[allow(unused)]
    pub(crate) fn store_in_basefee_ptr_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        basefee_ptr: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
    ) {
        block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::STORE_IN_BASEFEE_PTR),
                &[syscall_ctx, basefee_ptr],
                &[],
                location,
            ))
            .result(0);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn call_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
        gas: Value<'c, 'c>,
        address: Value<'c, 'c>,
        value_ptr: Value<'c, 'c>,
        args_offset: Value<'c, 'c>,
        args_size: Value<'c, 'c>,
        ret_offset: Value<'c, 'c>,
        ret_size: Value<'c, 'c>,
        available_gas: Value<'c, 'c>,
        remaining_gas_ptr: Value<'c, 'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let uint32 = IntegerType::new(mlir_ctx, 32).into();
        let result = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::CALL),
                &[
                    syscall_ctx,
                    gas,
                    address,
                    value_ptr,
                    args_offset,
                    args_size,
                    ret_offset,
                    ret_size,
                    available_gas,
                    remaining_gas_ptr,
                ],
                &[uint32],
                location,
            ))
            .result(0)?;

        Ok(result.into())
    }

    #[allow(unused)]
    pub(crate) fn store_in_balance_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        address: Value<'c, 'c>,
        balance: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        let ptr_type = pointer(mlir_ctx, 0);
        let value = block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::STORE_IN_BALANCE),
            &[syscall_ctx, address, balance],
            &[],
            location,
        ));
    }
}
