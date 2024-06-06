pub const MAX_STACK_SIZE: usize = 1024;
pub const GAS_COUNTER_GLOBAL: &str = "emv_mlir__gas_counter";
pub const STACK_BASEPTR_GLOBAL: &str = "emv_mlir__stack_baseptr";
pub const STACK_PTR_GLOBAL: &str = "emv_mlir__stack_ptr";
pub const MEMORY_PTR_GLOBAL: &str = "emv_mlir__memory_ptr";
pub const MEMORY_SIZE_GLOBAL: &str = "emv_mlir__memory_size";
pub const MAIN_ENTRYPOINT: &str = "main";

pub const REVERT_EXIT_CODE: u8 = 255;
pub const RETURN_EXIT_CODE: u8 = 0;

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
    pub const POP: i64 = 2;
    pub const PC: i64 = 2;
    pub const MSIZE: i64 = 2;
    pub const GAS: i64 = 2;
    pub const JUMPDEST: i64 = 1;
    pub const PUSH0: i64 = 2;
    pub const PUSHN: i64 = 3;
    pub const JUMP: i64 = 8;
    pub const DUPN: i64 = 3;
    pub const SWAPN: i64 = 3;
    pub const BYTE: i64 = 3;
    pub const CALLDATASIZE: i64 = 2;
}
