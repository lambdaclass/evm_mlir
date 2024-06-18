pub const MAX_STACK_SIZE: usize = 1024;
pub const GAS_COUNTER_GLOBAL: &str = "evm_mlir__gas_counter";
pub const STACK_BASEPTR_GLOBAL: &str = "evm_mlir__stack_baseptr";
pub const STACK_PTR_GLOBAL: &str = "evm_mlir__stack_ptr";
pub const MEMORY_PTR_GLOBAL: &str = "evm_mlir__memory_ptr";
pub const MEMORY_SIZE_GLOBAL: &str = "evm_mlir__memory_size";
pub const CALLDATA_PTR_GLOBAL: &str = "evm_mlir__calldata_ptr";
pub const CALLDATA_SIZE_GLOBAL: &str = "evm_mlir__calldata_size";
pub const MAIN_ENTRYPOINT: &str = "main";

/// Contains the gas costs of the EVM instructions
pub mod gas_cost {
    pub const MSTORE: i64 = 3;
    pub const MSTORE8: i64 = 3;
    pub const MLOAD: i64 = 3;
    pub const ADD: i64 = 3;
    pub const AND: i64 = 3;
    pub const EXP: i64 = 10;
    pub const LT: i64 = 3;
    pub const SGT: i64 = 3;
    pub const GT: i64 = 3;
    pub const EQ: i64 = 3;
    pub const ISZERO: i64 = 3;
    pub const OR: i64 = 3;
    pub const MUL: i64 = 5;
    pub const SUB: i64 = 3;
    pub const DIV: i64 = 5;
    pub const SDIV: i64 = 5;
    pub const MOD: i64 = 5;
    pub const SMOD: i64 = 5;
    pub const ADDMOD: i64 = 8;
    pub const MULMOD: i64 = 8;
    pub const SIGNEXTEND: i64 = 5;
    pub const SHL: i64 = 3;
    pub const SLT: i64 = 3;
    pub const XOR: i64 = 3;
    pub const SAR: i64 = 3;
    pub const CODESIZE: i64 = 2;
    pub const CHAINID: i64 = 2;
    pub const NUMBER: i64 = 2;
    pub const POP: i64 = 2;
    pub const PC: i64 = 2;
    pub const MSIZE: i64 = 2;
    pub const GAS: i64 = 2;
    pub const JUMPDEST: i64 = 1;
    pub const MCOPY: i64 = 3;
    pub const PUSH0: i64 = 2;
    pub const PUSHN: i64 = 3;
    pub const JUMP: i64 = 8;
    pub const DUPN: i64 = 3;
    pub const SWAPN: i64 = 3;
    pub const BYTE: i64 = 3;
    pub const CALLDATALOAD: i64 = 3;
    pub const CALLDATASIZE: i64 = 2;
    pub const CALLVALUE: i64 = 2;
    pub const JUMPI: i64 = 10;
    pub const LOG: i64 = 375;
    pub const GASPRICE: i64 = 2;

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
}
