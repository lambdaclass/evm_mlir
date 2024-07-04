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

use crate::{
    db::{AccountInfo, Database, Db},
    env::{Env, TransactTo},
    primitives::{Address, B256, U256 as EU256},
    result::{EVMError, ExecutionResult, HaltReason, Output, ResultAndState, SuccessReason},
    state::EvmStorageSlot,
    utils::u256_from_u128,
};
use melior::ExecutionEngine;
use sha3::{Digest, Keccak256};
use std::collections::HashMap;

/// Function type for the main entrypoint of the generated code
pub type MainFunc = extern "C" fn(&mut SyscallContext, initial_gas: u64) -> u8;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(C, align(16))]
pub struct U256 {
    pub lo: u128,
    pub hi: u128,
}

impl U256 {
    pub fn from_fixed_be_bytes(bytes: [u8; 32]) -> Self {
        let hi = u128::from_be_bytes(bytes[0..16].try_into().unwrap());
        let lo = u128::from_be_bytes(bytes[16..32].try_into().unwrap());
        U256 { hi, lo }
    }

    pub fn copy_from(&mut self, value: &Address) {
        let mut buffer = [0u8; 32];
        buffer[12..32].copy_from_slice(&value.0);
        self.lo = u128::from_be_bytes(buffer[16..32].try_into().unwrap());
        self.hi = u128::from_be_bytes(buffer[0..16].try_into().unwrap());
    }
}

impl TryFrom<&U256> for Address {
    type Error = ();

    fn try_from(value: &U256) -> Result<Self, Self::Error> {
        const FIRST_12_BYTES_MASK: u128 = 0xFFFFFFFFFFFFFFFFFFFFFFFF00000000;
        let hi_bytes = value.hi.to_be_bytes();
        let lo_bytes = value.lo.to_be_bytes();
        // Address is valid only if first 12 bytes are set to zero
        if value.hi & FIRST_12_BYTES_MASK != 0 {
            return Err(());
        }
        let address = [&hi_bytes[12..16], &lo_bytes[..]].concat();
        Ok(Address::from_slice(&address))
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
    gas_refund: u64,
    exit_status: Option<ExitStatusCode>,
    logs: Vec<LogData>,
    journaled_storage: HashMap<EU256, EvmStorageSlot>, // TODO: rename to journaled_state and move into a separate Struct
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
        let gas_refunded = self.inner_context.gas_refund;
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
                gas_refunded,
                output: Output::Call(return_values.into()), // TODO: add case Output::Create
                logs: self.logs(),
            },
            ExitStatusCode::Stop => ExecutionResult::Success {
                reason: SuccessReason::Stop,
                gas_used,
                gas_refunded,
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

        let mut state = self.db.clone().into_state();

        let caller_account = state.entry(self.env.tx.caller).or_default();
        caller_account
            .storage
            .extend(self.inner_context.journaled_storage.clone());

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
        *hash_ptr = U256::from_fixed_be_bytes(result.into());
    }

    pub extern "C" fn store_in_callvalue_ptr(&self, value: &mut U256) {
        let aux = &self.env.tx.value;
        value.lo = aux.low_u128();
        value.hi = (aux >> 128).low_u128();
    }

    pub extern "C" fn store_in_blobbasefee_ptr(&self, value: &mut u128) {
        *value = self.env.block.blob_gasprice.unwrap_or_default();
    }

    pub extern "C" fn get_gaslimit(&self) -> u64 {
        self.env.tx.gas_limit
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
        address.copy_from(aux);
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

        let Some(code_slice) = &self
            .inner_context
            .program
            .get(code_offset..code_offset + size)
        else {
            eprintln!("Error on copy_code_to_memory");
            return; // TODO: fix bug with code indexes
        };
        // copy the program into memory
        self.inner_context.memory[dest_offset..dest_offset + size].copy_from_slice(code_slice);
    }

    pub extern "C" fn read_storage(&mut self, stg_key: &U256, stg_value: &mut U256) {
        let address = self.env.tx.caller;

        let key = u256_from_u128(stg_key.hi, stg_key.lo);

        // Read value from journaled_storage. If there isn't one, then read from db
        let result = self
            .inner_context
            .journaled_storage
            .get(&key)
            .map(|slot| slot.present_value)
            .unwrap_or_else(|| self.db.read_storage(address, key));

        stg_value.hi = (result >> 128).low_u128();
        stg_value.lo = result.low_u128();
    }

    pub extern "C" fn write_storage(&mut self, stg_key: &U256, stg_value: &mut U256) -> i64 {
        let key = u256_from_u128(stg_key.hi, stg_key.lo);
        let value = u256_from_u128(stg_value.hi, stg_value.lo);

        // Update the journaled storage and retrieve the previous stored values.
        let (original, current, is_cold) = match self.inner_context.journaled_storage.get_mut(&key)
        {
            Some(slot) => {
                let current_value = slot.present_value;
                let is_cold = slot.is_cold;

                slot.present_value = value;
                slot.is_cold = false;

                (slot.original_value, current_value, is_cold)
            }
            None => {
                let original_value = self.db.read_storage(self.env.tx.caller, key);
                self.inner_context.journaled_storage.insert(
                    key,
                    EvmStorageSlot {
                        original_value,
                        present_value: value,
                        is_cold: false,
                    },
                );
                (original_value, original_value, true)
            }
        };

        // Compute the gas cost
        let mut gas_cost: i64 = if original.is_zero() && current.is_zero() && current != value {
            20_000
        } else if original == current && current != value {
            2_900
        } else {
            100
        };

        // When the value is cold, add extra 2100 gas
        if is_cold {
            gas_cost += 2_100;
        }

        // Compute the gas refund
        let reset_non_zero_to_zero = !original.is_zero() && !current.is_zero() && value.is_zero();
        let undo_reset_to_zero = !original.is_zero() && current.is_zero() && !value.is_zero();
        let undo_reset_to_zero_into_original = undo_reset_to_zero && (value == original);
        let reset_back_to_zero = original.is_zero() && !current.is_zero() && value.is_zero();
        let reset_to_original = (current != value) && (original == value);

        let gas_refund: i64 = if reset_non_zero_to_zero {
            4_800
        } else if undo_reset_to_zero_into_original {
            -2_000
        } else if undo_reset_to_zero {
            -4_800
        } else if reset_back_to_zero {
            19_900
        } else if reset_to_original {
            2_800
        } else {
            0
        };

        if gas_refund > 0 {
            self.inner_context.gas_refund += gas_refund as u64;
        } else {
            self.inner_context.gas_refund -= gas_refund.unsigned_abs();
        };

        gas_cost
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

    pub extern "C" fn get_block_hash(&mut self, number: &mut U256) {
        let number_as_u256 = u256_from_u128(number.hi, number.lo);

        // If number is not in the valid range (last 256 blocks), return zero.
        let hash = if number_as_u256 < self.env.block.number.saturating_sub(EU256::from(256))
            || number_as_u256 >= self.env.block.number
        {
            // TODO: check if this is necessary. Db should only contain last 256 blocks, so number check would not be needed.
            B256::zero()
        } else {
            self.db.block_hash(number_as_u256).unwrap_or(B256::zero())
        };

        let (hi, lo) = hash.as_bytes().split_at(16);
        number.lo = u128::from_be_bytes(lo.try_into().unwrap());
        number.hi = u128::from_be_bytes(hi.try_into().unwrap());
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

    pub extern "C" fn get_codesize_from_address(&mut self, address: &U256) -> u64 {
        Address::try_from(address)
            .map(|a| self.db.code_by_address(a).len())
            .unwrap_or(0) as _
    }

    pub extern "C" fn get_address_ptr(&mut self) -> *const u8 {
        self.env.tx.get_address().to_fixed_bytes().as_ptr()
    }

    pub extern "C" fn get_prevrandao(&self, prevrandao: &mut U256) {
        let randao = self.env.block.prevrandao.unwrap_or_default();
        *prevrandao = U256::from_fixed_be_bytes(randao.into());
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

    pub extern "C" fn get_blob_hash_at_index(&mut self, index: &U256, blobhash: &mut U256) {
        if index.hi != 0 {
            *blobhash = U256::default();
            return;
        }
        *blobhash = usize::try_from(index.lo)
            .ok()
            .and_then(|idx| self.env.tx.blob_hashes.get(idx).cloned())
            .map(|x| U256::from_fixed_be_bytes(x.into()))
            .unwrap_or_default();
    }

    pub extern "C" fn copy_ext_code_to_memory(
        &mut self,
        address_value: &U256,
        code_offset: u32,
        size: u32,
        dest_offset: u32,
    ) {
        let size = size as usize;
        let code_offset = code_offset as usize;
        let dest_offset = dest_offset as usize;
        let Ok(address) = Address::try_from(address_value) else {
            self.inner_context.memory[dest_offset..dest_offset + size].fill(0);
            return;
        };
        let code = self.db.code_by_address(address);
        let code_size = code.len();
        let code_to_copy_size = code_size.saturating_sub(code_offset);
        let code_slice = &code[code_offset..code_offset + code_to_copy_size];
        let padding_size = size - code_to_copy_size;
        let padding_offset = dest_offset + code_to_copy_size;
        // copy the program into memory
        self.inner_context.memory[dest_offset..dest_offset + code_to_copy_size]
            .copy_from_slice(code_slice);
        // pad the left part with zero
        self.inner_context.memory[padding_offset..padding_offset + padding_size].fill(0);
    }
}

pub mod symbols {
    pub const WRITE_RESULT: &str = "evm_mlir__write_result";
    pub const EXTEND_MEMORY: &str = "evm_mlir__extend_memory";
    pub const KECCAK256_HASHER: &str = "evm_mlir__keccak256_hasher";
    pub const STORAGE_WRITE: &str = "evm_mlir__write_storage";
    pub const STORAGE_READ: &str = "evm_mlir__read_storage";
    pub const APPEND_LOG: &str = "evm_mlir__append_log";
    pub const APPEND_LOG_ONE_TOPIC: &str = "evm_mlir__append_log_with_one_topic";
    pub const APPEND_LOG_TWO_TOPICS: &str = "evm_mlir__append_log_with_two_topics";
    pub const APPEND_LOG_THREE_TOPICS: &str = "evm_mlir__append_log_with_three_topics";
    pub const APPEND_LOG_FOUR_TOPICS: &str = "evm_mlir__append_log_with_four_topics";
    pub const GET_CALLDATA_PTR: &str = "evm_mlir__get_calldata_ptr";
    pub const GET_CALLDATA_SIZE: &str = "evm_mlir__get_calldata_size";
    pub const GET_CODESIZE_FROM_ADDRESS: &str = "evm_mlir__get_codesize_from_address";
    pub const COPY_CODE_TO_MEMORY: &str = "evm_mlir__copy_code_to_memory";
    pub const GET_ADDRESS_PTR: &str = "evm_mlir__get_address_ptr";
    pub const GET_GASLIMIT: &str = "evm_mlir__get_gaslimit";
    pub const STORE_IN_CALLVALUE_PTR: &str = "evm_mlir__store_in_callvalue_ptr";
    pub const STORE_IN_BLOBBASEFEE_PTR: &str = "evm_mlir__store_in_blobbasefee_ptr";
    pub const GET_BLOB_HASH_AT_INDEX: &str = "evm_mlir__get_blob_hash_at_index";
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
    pub const COPY_EXT_CODE_TO_MEMORY: &str = "evm_mlir__copy_ext_code_to_memory";
    pub const GET_PREVRANDAO: &str = "evm_mlir__get_prevrandao";
    pub const GET_BLOCK_HASH: &str = "evm_mlir__get_block_hash";
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
            symbols::STORAGE_WRITE,
            SyscallContext::write_storage as *const fn(*mut c_void, *const U256, *const U256)
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
                as *const extern "C" fn(&SyscallContext, *mut u128) -> () as *mut (),
        );
        engine.register_symbol(
            symbols::GET_CODESIZE_FROM_ADDRESS,
            SyscallContext::get_codesize_from_address as *const fn(*mut c_void, *mut U256)
                as *mut (),
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
            symbols::GET_GASLIMIT,
            SyscallContext::get_gaslimit as *const fn(*mut c_void) as *mut (),
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
            symbols::GET_PREVRANDAO,
            SyscallContext::get_prevrandao as *const fn(*mut c_void, *mut U256) as *mut (),
        );
        engine.register_symbol(
            symbols::GET_BLOB_HASH_AT_INDEX,
            SyscallContext::get_blob_hash_at_index as *const fn(*mut c_void, *mut U256, *mut U256)
                as *mut (),
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
        engine.register_symbol(
            symbols::COPY_EXT_CODE_TO_MEMORY,
            SyscallContext::copy_ext_code_to_memory
                as *const extern "C" fn(*mut c_void, *mut U256, u32, u32, u32)
                as *mut (),
        );
        engine.register_symbol(
            symbols::GET_BLOCK_HASH,
            SyscallContext::get_block_hash as *const fn(*mut c_void, *mut U256) as *mut (),
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
            StringAttribute::new(context, symbols::GET_GASLIMIT),
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
            StringAttribute::new(context, symbols::STORAGE_WRITE),
            r#TypeAttribute::new(
                FunctionType::new(context, &[ptr_type, ptr_type, ptr_type], &[uint64]).into(),
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
            StringAttribute::new(context, symbols::GET_CODESIZE_FROM_ADDRESS),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[uint64]).into()),
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
            StringAttribute::new(context, symbols::GET_PREVRANDAO),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
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
            StringAttribute::new(context, symbols::STORE_IN_BALANCE),
            TypeAttribute::new(
                FunctionType::new(context, &[ptr_type, ptr_type, ptr_type], &[]).into(),
            ),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::COPY_EXT_CODE_TO_MEMORY),
            TypeAttribute::new(
                FunctionType::new(context, &[ptr_type, ptr_type, uint32, uint32, uint32], &[])
                    .into(),
            ),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::GET_BLOB_HASH_AT_INDEX),
            TypeAttribute::new(
                FunctionType::new(context, &[ptr_type, ptr_type, ptr_type], &[]).into(),
            ),
            Region::new(),
            attributes,
            location,
        ));

        module.body().append_operation(func::func(
            context,
            StringAttribute::new(context, symbols::GET_BLOCK_HASH),
            TypeAttribute::new(FunctionType::new(context, &[ptr_type, ptr_type], &[]).into()),
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

    pub(crate) fn get_gaslimit<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        location: Location<'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let uint64 = IntegerType::new(mlir_ctx, 64).into();
        let value = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_GASLIMIT),
                &[syscall_ctx],
                &[uint64],
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

    /// Writes the storage given a key value pair
    pub(crate) fn storage_write_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        key: Value<'c, 'c>,
        value: Value<'c, 'c>,
        location: Location<'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let uint64 = IntegerType::new(mlir_ctx, 64);
        let value = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::STORAGE_WRITE),
                &[syscall_ctx, key, value],
                &[uint64.into()],
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

    /// Receives an account address and copies the corresponding bytecode
    /// to memory.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn copy_ext_code_to_memory_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        address_ptr: Value<'c, 'c>,
        offset: Value<'c, 'c>,
        size: Value<'c, 'c>,
        dest_offset: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::COPY_EXT_CODE_TO_MEMORY),
            &[syscall_ctx, address_ptr, offset, size, dest_offset],
            &[],
            location,
        ));
    }

    pub(crate) fn get_prevrandao_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        prevrandao_ptr: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_PREVRANDAO),
            &[syscall_ctx, prevrandao_ptr],
            &[],
            location,
        ));
    }

    pub(crate) fn get_codesize_from_address_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        address: Value<'c, 'c>,
        location: Location<'c>,
    ) -> Result<Value<'c, 'c>, CodegenError> {
        let uint64 = IntegerType::new(mlir_ctx, 64).into();
        let value = block
            .append_operation(func::call(
                mlir_ctx,
                FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_CODESIZE_FROM_ADDRESS),
                &[syscall_ctx, address],
                &[uint64],
                location,
            ))
            .result(0)?;
        Ok(value.into())
    }

    pub(crate) fn get_blob_hash_at_index_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        index: Value<'c, 'c>,
        blobhash: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_BLOB_HASH_AT_INDEX),
            &[syscall_ctx, index, blobhash],
            &[],
            location,
        ));
    }

    pub(crate) fn get_block_hash_syscall<'c>(
        mlir_ctx: &'c MeliorContext,
        syscall_ctx: Value<'c, 'c>,
        block: &'c Block,
        block_number: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        block.append_operation(func::call(
            mlir_ctx,
            FlatSymbolRefAttribute::new(mlir_ctx, symbols::GET_BLOCK_HASH),
            &[syscall_ctx, block_number],
            &[],
            location,
        ));
    }
}
