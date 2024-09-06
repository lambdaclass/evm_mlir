use thiserror::Error;

pub const MAX_STACK_SIZE: usize = 1024;
pub const GAS_COUNTER_GLOBAL: &str = "evm_mlir__gas_counter";
pub const STACK_BASEPTR_GLOBAL: &str = "evm_mlir__stack_baseptr";
pub const CODE_PTR_GLOBAL: &str = "evm_mlir__code_ptr";
pub const STACK_PTR_GLOBAL: &str = "evm_mlir__stack_ptr";
pub const MEMORY_PTR_GLOBAL: &str = "evm_mlir__memory_ptr";
pub const MEMORY_SIZE_GLOBAL: &str = "evm_mlir__memory_size";
pub const CALLDATA_PTR_GLOBAL: &str = "evm_mlir__calldata_ptr";
pub const CALLDATA_SIZE_GLOBAL: &str = "evm_mlir__calldata_size";
pub const MAIN_ENTRYPOINT: &str = "main";
pub const TRUSTED_SETUP_PATH: &str = "official_trusted_setup.txt";

// An empty bytecode has the following Keccak256 hash
pub const EMPTY_CODE_HASH_STR: &str =
    "0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470";

pub const VERSIONED_HASH_VERSION_KZG: u8 = 0x01;
pub const MAX_BLOB_NUMBER_PER_BLOCK: u8 = 0x01;

//TODO: Add missing opcodes gas consumption costs
//  -> This implies refactoring codegen/operations.rs
/// Contains the gas costs of the EVM instructions
pub mod gas_cost {
    pub const ADD: i64 = 3;
    pub const MUL: i64 = 5;
    pub const SUB: i64 = 3;
    pub const DIV: i64 = 5;
    pub const SDIV: i64 = 5;
    pub const MOD: i64 = 5;
    pub const SMOD: i64 = 5;
    pub const ADDMOD: i64 = 8;
    pub const MULMOD: i64 = 8;
    pub const EXP: i64 = 10;
    pub const SIGNEXTEND: i64 = 5;
    pub const LT: i64 = 3;
    pub const GT: i64 = 3;
    pub const SLT: i64 = 3;
    pub const SGT: i64 = 3;
    pub const EQ: i64 = 3;
    pub const ISZERO: i64 = 3;
    pub const AND: i64 = 3;
    pub const OR: i64 = 3;
    pub const XOR: i64 = 3;
    pub const NOT: i64 = 3;
    pub const BYTE: i64 = 3;
    pub const SHL: i64 = 3;
    pub const SAR: i64 = 3;
    pub const BALANCE: i64 = 100;
    pub const ORIGIN: i64 = 2;
    pub const CALLER: i64 = 2;
    pub const CALLVALUE: i64 = 2;
    pub const CALLDATALOAD: i64 = 3;
    pub const CALLDATASIZE: i64 = 2;
    pub const CALLDATACOPY: i64 = 3;
    pub const CODESIZE: i64 = 2;
    pub const COINBASE: i64 = 2;
    pub const GASPRICE: i64 = 2;
    pub const SELFBALANCE: i64 = 5;
    pub const NUMBER: i64 = 2;
    pub const PREVRANDAO: i64 = 2;
    pub const BLOBBASEFEE: i64 = 2;
    pub const CHAINID: i64 = 2;
    pub const BASEFEE: i64 = 2;
    pub const BLOBHASH: i64 = 3;
    pub const POP: i64 = 2;
    pub const MLOAD: i64 = 3;
    pub const MSTORE: i64 = 3;
    pub const MSTORE8: i64 = 3;
    pub const SLOAD_WARM: i64 = 100;
    pub const SLOAD_COLD: i64 = 2100;
    pub const JUMP: i64 = 8;
    pub const JUMPI: i64 = 10;
    pub const PC: i64 = 2;
    pub const MSIZE: i64 = 2;
    pub const GAS: i64 = 2;
    pub const JUMPDEST: i64 = 1;
    pub const MCOPY: i64 = 3;
    pub const PUSH0: i64 = 2;
    pub const PUSHN: i64 = 3;
    pub const DUPN: i64 = 3;
    pub const SWAPN: i64 = 3;
    pub const TIMESTAMP: i64 = 2;
    pub const KECCAK256: i64 = 30;
    pub const CODECOPY: i64 = 3;
    pub const LOG: i64 = 375;
    pub const BLOCKHASH: i64 = 20;
    pub const CALL: i64 = 0;
    pub const EXTCODESIZE_WARM: i64 = 100;
    pub const EXTCODECOPY_WARM: i64 = 100;
    pub const RETURNDATASIZE: i64 = 2;
    pub const RETURNDATACOPY: i64 = 3;
    pub const ADDRESS: i64 = 2;
    pub const GASLIMIT: i64 = 2;
    pub const SSTORE_MIN_REMAINING_GAS: i64 = 2_300;
    pub const CREATE: i64 = 32_000;
    pub const TLOAD: i64 = 100;
    pub const TSTORE: i64 = 100;
    pub const SELFDESTRUCT: i64 = 5_000;
    pub const SELFDESTRUCT_DYNAMIC_GAS: i64 = 25_000;

    pub const MIN_BLOB_GASPRICE: u64 = 1;
    pub const BLOB_GASPRICE_UPDATE_FRACTION: u64 = 3338477;

    pub const BYTE_DEPOSIT_COST: i64 = 200;
    pub const INIT_WORD_COST: i64 = 2;
    pub const HASH_WORD_COST: i64 = 6;

    // Transaction costs
    pub const TX_BASE_COST: u64 = 21000;
    pub const TX_DATA_COST_PER_NON_ZERO: u64 = 16;
    pub const TX_DATA_COST_PER_ZERO: u64 = 4;
    pub const TX_CREATE_COST: u64 = 32000;
    pub const TX_ACCESS_LIST_ADDRESS_COST: u64 = 2400;
    pub const TX_ACCESS_LIST_STORAGE_KEY_COST: u64 = 1900;
    pub const MAX_CODE_SIZE: usize = 0x6000;

    pub fn init_code_cost(init_code_length: usize) -> u64 {
        INIT_WORD_COST as u64 * (init_code_length as u64 + 31) / 32
    }

    pub fn memory_expansion_cost(last_size: u32, new_size: u32) -> i64 {
        let new_memory_size_word = (new_size + 31) / 32;
        let new_memory_cost =
            (new_memory_size_word * new_memory_size_word) / 512 + (3 * new_memory_size_word);
        let last_memory_size_word = (last_size + 31) / 32;
        let last_memory_cost =
            (last_memory_size_word * last_memory_size_word) / 512 + (3 * last_memory_size_word);
        (new_memory_cost - last_memory_cost).into()
    }

    pub fn memory_copy_cost(size: u32) -> i64 {
        let memory_word_size = (size + 31) / 32;

        (memory_word_size * 3).into()
    }
    pub fn log_dynamic_gas_cost(size: u32, topic_count: u32) -> i64 {
        (super::gas_cost::LOG * topic_count as i64) + (8 * size as i64)
    }

    fn exponent_byte_size(exponent: u64) -> i64 {
        (((64 - exponent.leading_zeros()) + 7) / 8).into()
    }

    pub fn exp_dynamic_cost(exponent: u64) -> i64 {
        10 + 50 * exponent_byte_size(exponent)
    }
}

pub mod call_opcode {
    // Return codes
    pub const SUCCESS_RETURN_CODE: u8 = 1;
    pub const REVERT_RETURN_CODE: u8 = 0;

    // Gas related constants
    pub const WARM_MEMORY_ACCESS_COST: u64 = 100;
    pub const NOT_ZERO_VALUE_COST: u64 = 9000;
    pub const EMPTY_CALLEE_COST: u64 = 25000;
    pub const STIPEND_GAS_ADDITION: u64 = 2300;
    pub const GAS_CAP_DIVISION_FACTOR: u64 = 64;
}

pub mod precompiles {
    pub const ECRECOVER_COST: u64 = 3000;
    pub const ECRECOVER_ADDRESS: u64 = 0x01;
    pub const SHA2_256_COST: u64 = 60;
    pub const SHA2_256_ADDRESS: u64 = 0x02;
    pub const RIPEMD_160_COST: u64 = 600;
    pub const RIPEMD_160_ADDRESS: u64 = 0x03;
    pub const IDENTITY_COST: u64 = 15;
    pub const IDENTITY_ADDRESS: u64 = 0x04;
    pub const MODEXP_ADDRESS: u64 = 0x05;
    pub const ECADD_ADDRESS: u64 = 0x06;
    pub const ECADD_COST: u64 = 150;
    pub const ECMUL_ADDRESS: u64 = 0x07;
    pub const ECMUL_COST: u64 = 6000;
    pub const ECPAIRING_ADDRESS: u64 = 0x08;
    pub const ECPAIRING_STATIC_COST: u64 = 45000;
    pub const ECPAIRING_PAIRING_COST: u64 = 34000;
    pub const BLAKE2F_ADDRESS: u64 = 0x09;

    pub fn identity_dynamic_cost(len: u64) -> u64 {
        (len + 31) / 32 * 3
    }

    pub fn sha2_256_dynamic_cost(len: u64) -> u64 {
        (len + 31) / 32 * 12
    }

    pub fn ripemd_160_dynamic_cost(len: u64) -> u64 {
        (len + 31) / 32 * 120
    }

    pub fn ecpairing_dynamic_cost(len: u64) -> u64 {
        ECPAIRING_PAIRING_COST * (len / 192)
    }

    pub fn blake2_gas_cost(rounds: u32) -> u64 {
        rounds as u64
    }
}

#[derive(PartialEq, Debug)]
pub enum CallType {
    Call,
    StaticCall,
    DelegateCall,
    CallCode,
}

#[derive(Error, Debug)]
#[error("Couldn't parse CallType from u8")]
pub struct CallTypeParseError;

impl TryFrom<u8> for CallType {
    type Error = CallTypeParseError;
    fn try_from(call_type: u8) -> Result<CallType, Self::Error> {
        match call_type {
            x if x == CallType::Call as u8 => Ok(CallType::Call),
            x if x == CallType::StaticCall as u8 => Ok(CallType::StaticCall),
            x if x == CallType::DelegateCall as u8 => Ok(CallType::DelegateCall),
            x if x == CallType::CallCode as u8 => Ok(CallType::CallCode),
            _ => Err(CallTypeParseError),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exp_dynamic_gas_cost() {
        assert_eq!(gas_cost::exp_dynamic_cost(255), 60);
        assert_eq!(gas_cost::exp_dynamic_cost(256), 110);
        assert_eq!(gas_cost::exp_dynamic_cost(65536), 160);
        assert_eq!(gas_cost::exp_dynamic_cost(16777216), 210);
        assert_eq!(gas_cost::exp_dynamic_cost(4294967296), 260);
    }
}
