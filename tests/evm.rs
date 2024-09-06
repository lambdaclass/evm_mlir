use rstest::rstest;
use sha3::{Digest, Keccak256};
use std::{collections::HashMap, str::FromStr};

use evm_mlir::{
    constants::{
        call_opcode, gas_cost,
        precompiles::{
            BLAKE2F_ADDRESS, ECADD_ADDRESS, ECMUL_ADDRESS, ECPAIRING_ADDRESS, ECRECOVER_ADDRESS,
            IDENTITY_ADDRESS, MODEXP_ADDRESS, RIPEMD_160_ADDRESS, SHA2_256_ADDRESS,
        },
        EMPTY_CODE_HASH_STR,
    },
    db::{Bytecode, Database, Db},
    env::TransactTo,
    primitives::{Address, Bytes, B256, U256 as EU256},
    program::{Operation, Program},
    syscall::{LogData, GAS_REFUND_DENOMINATOR, U256},
    utils::compute_contract_address2,
    Env, Evm,
};

use num_bigint::BigUint;

fn append_return_result_operations(operations: &mut Vec<Operation>) {
    operations.extend([
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ]);
}

fn default_env_and_db_setup(operations: Vec<Operation>) -> (Env, Db) {
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    let program = Program::from(operations);
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    (env, db)
}

fn run_program_assert_num_result(env: Env, db: Db, expected_result: BigUint) {
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());
    let result_data = BigUint::from_bytes_be(result.output().unwrap_or(&Bytes::new()));
    assert_eq!(result_data, expected_result);
}

fn run_program_assert_bytes_result(env: Env, db: Db, expected_result: &[u8]) {
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());
    assert_eq!(result.output().unwrap().as_ref(), expected_result);
}

fn run_program_assert_halt(env: Env, db: Db) {
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();

    assert!(result.is_halt());
}

fn run_program_assert_gas_exact_with_db(mut env: Env, db: Db, needed_gas: u64) {
    // Ok run
    env.tx.gas_limit = needed_gas + gas_cost::TX_BASE_COST;
    let mut evm = Evm::new(env.clone(), db.clone());
    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());

    // Halt run
    env.tx.gas_limit = needed_gas - 1 + gas_cost::TX_BASE_COST;
    let mut evm = Evm::new(env.clone(), db);
    let result = evm.transact_commit().unwrap();
    assert!(result.is_halt());
}

fn run_program_assert_gas_exact(operations: Vec<Operation>, env: Env, needed_gas: u64) {
    let address = env.tx.get_address();

    //Ok run
    let program = Program::from(operations.clone());
    let mut env_success = env.clone();
    env_success.tx.gas_limit = needed_gas + gas_cost::TX_BASE_COST;
    let db = Db::new().with_contract(address, program.to_bytecode().into());
    let mut evm = Evm::new(env_success, db);

    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());

    //Halt run
    let program = Program::from(operations.clone());
    let mut env_halt = env.clone();
    env_halt.tx.gas_limit = needed_gas - 1 + gas_cost::TX_BASE_COST;
    let db = Db::new().with_contract(address, program.to_bytecode().into());
    let mut evm = Evm::new(env_halt, db);

    let result = evm.transact_commit().unwrap();
    assert!(result.is_halt());
}

fn run_program_assert_gas_and_refund(
    mut env: Env,
    db: Db,
    needed_gas: u64,
    used_gas: u64,
    refunded_gas: u64,
) {
    env.tx.gas_limit = needed_gas + gas_cost::TX_BASE_COST;
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());
    assert_eq!(result.gas_used(), used_gas);
    assert_eq!(result.gas_refunded(), refunded_gas);
}

fn get_fibonacci_program(n: u64) -> Vec<Operation> {
    assert!(n > 0, "n must be greater than 0");

    let main_loop_pc = 36;
    let end_pc = 57;
    vec![
        Operation::Push((32, (n - 1).into())),     // 0-32
        Operation::Push0,                          // fib(0)
        Operation::Push((1, BigUint::from(1_u8))), // fib(1)
        // main loop
        Operation::Jumpdest { pc: main_loop_pc }, // 35
        Operation::Dup(3),
        Operation::IsZero,
        Operation::Push((1, BigUint::from(end_pc))), // 38-39
        Operation::Jumpi,
        // fib(n-1) + fib(n-2)
        Operation::Dup(2),
        Operation::Dup(2),
        Operation::Add,
        // [fib(n-2), fib(n-1), fib(n)] -> [fib(n-1) + fib(n)]
        Operation::Swap(2),
        Operation::Pop,
        Operation::Swap(1),
        // decrement counter
        Operation::Swap(2),
        Operation::Push((1, BigUint::from(1_u8))), // 48-49
        Operation::Swap(1),
        Operation::Sub,
        Operation::Swap(2),
        Operation::Push((1, BigUint::from(main_loop_pc))), // 53-54
        Operation::Jump,
        Operation::Jumpdest { pc: end_pc },
        Operation::Swap(2),
        Operation::Pop,
        Operation::Pop,
        // Return the requested fibonacci element
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ]
}

#[test]
fn fibonacci_example() {
    let operations = get_fibonacci_program(10);
    let program = Program::from(operations);

    let mut env = Env::default();
    env.tx.gas_limit = 999_999;

    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(result.is_success());
    let number = BigUint::from_bytes_be(result.output().unwrap());
    assert_eq!(number, 55_u32.into());
}

#[test]
fn block_hash_happy_path() {
    let block_number = 1_u8;
    let block_hash = 209433;
    let current_block_number = 3_u8;
    let expected_block_hash = BigUint::from(block_hash);
    let mut operations = vec![
        Operation::Push((1, BigUint::from(block_number))),
        Operation::BlockHash,
    ];
    append_return_result_operations(&mut operations);
    let (mut env, mut db) = default_env_and_db_setup(operations);
    env.block.number = EU256::from(current_block_number);
    db.insert_block_hash(EU256::from(block_number), B256::from_low_u64_be(block_hash));

    run_program_assert_num_result(env, db, expected_block_hash);
}

#[test]
fn block_hash_with_current_block_number() {
    let block_number = 1_u8;
    let block_hash = 29293;
    let current_block_number = block_number;
    let expected_block_hash = BigUint::ZERO;
    let mut operations = vec![
        Operation::Push((1, BigUint::from(block_number))),
        Operation::BlockHash,
    ];
    append_return_result_operations(&mut operations);
    let (mut env, mut db) = default_env_and_db_setup(operations);
    env.block.number = EU256::from(current_block_number);
    db.insert_block_hash(EU256::from(block_number), B256::from_low_u64_be(block_hash));

    run_program_assert_num_result(env, db, expected_block_hash);
}

#[test]
fn block_hash_with_stack_underflow() {
    let operations = vec![Operation::BlockHash];
    let (env, db) = default_env_and_db_setup(operations);

    run_program_assert_halt(env, db);
}

#[test]
fn test_opcode_origin() {
    let mut operations = vec![Operation::Origin];
    append_return_result_operations(&mut operations);
    let mut env = Env::default();
    let caller = Address::from_str("0x9bbfed6889322e016e0a02ee459d306fc19545d8").unwrap();
    env.tx.caller = caller;
    env.tx.gas_limit = 999_999;
    let program = Program::from(operations);
    let bytecode = Bytecode::from(program.to_bytecode());
    let db = Db::new().with_contract(Address::zero(), bytecode);
    let caller_bytes = &caller.to_fixed_bytes();
    //We extend the result to be 32 bytes long.
    let expected_result: [u8; 32] = [&[0u8; 12], &caller_bytes[0..20]]
        .concat()
        .try_into()
        .unwrap();
    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn test_opcode_origin_gas_check() {
    let operations = vec![Operation::Origin];
    let needed_gas = gas_cost::ORIGIN;
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn test_opcode_origin_with_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Origin);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn calldataload_with_all_bytes_before_end_of_calldata() {
    // in this case offset + 32 < calldata_size
    // calldata is
    //       index =    0  1  ... 30 31 30  ... 63
    //      calldata = [0, 0, ..., 0, 1, 0, ..., 0]
    // the offset is 0 and given that the slice width is always 32,
    // then the result is
    //      calldata_slice = [0, 0, ..., 1]
    let calldata_offset = 0_u8;
    let memory_offset = 0_u8;
    let size = 32_u8;
    let program = Program::from(vec![
        Operation::Push((1_u8, BigUint::from(calldata_offset))),
        Operation::CalldataLoad,
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Mstore,
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Return,
    ]);

    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    let mut calldata = vec![0x00; 64];
    calldata[31] = 1;
    env.tx.data = Bytes::from(calldata);
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(result.is_success());
    let calldata_slice = result.output().unwrap();
    let mut expected_result = [0_u8; 32];
    expected_result[31] = 1;
    assert_eq!(calldata_slice.as_ref(), expected_result);
}

#[test]
fn calldataload_with_some_bytes_after_end_of_calldata() {
    // in this case offset + 32 >= calldata_size
    // the calldata is
    //       index =    0  1  ... 30 31
    //      calldata = [0, 0, ..., 0, 1]
    // and the offset is 1, given that in the result all bytes after
    // calldata end are set to 0, then the result is
    //      calldata_slice = [0, ..., 0, 1, 0]
    let calldata_offset = 1_u8;
    let memory_offset = 0_u8;
    let size = 32_u8;
    let program = Program::from(vec![
        Operation::Push((1_u8, BigUint::from(calldata_offset))),
        Operation::CalldataLoad,
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Mstore,
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Return,
    ]);

    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    let mut calldata = vec![0x00; 32];
    calldata[31] = 1;
    env.tx.data = Bytes::from(calldata);
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(result.is_success());
    let calldata_slice = result.output().unwrap();
    let mut expected_result = [0_u8; 32];
    expected_result[30] = 1;
    assert_eq!(calldata_slice.as_ref(), expected_result);
}

#[test]
fn calldataload_with_offset_greater_than_calldata_size() {
    // in this case offset > calldata_size
    // the calldata is
    //       index =    0  1  ... 30 31
    //      calldata = [1, 1, ..., 1, 1]
    // and the offset is 64, given that in the result all bytes after
    // calldata end are set to 0, then the result is
    //      calldata_slice = [0, ..., 0, 0, 0]
    let calldata_offset = 64_u8;
    let memory_offset = 0_u8;
    let size = 32_u8;
    let program = Program::from(vec![
        Operation::Push((1_u8, BigUint::from(calldata_offset))),
        Operation::CalldataLoad,
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Mstore,
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Return,
    ]);

    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.data = Bytes::from(vec![0xff; 32]);
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(result.is_success());
    let calldata_slice = result.output().unwrap();
    let expected_result = [0_u8; 32];
    assert_eq!(calldata_slice.as_ref(), expected_result);
}

#[test]
fn test_calldatacopy() {
    let operations = vec![
        Operation::Push((1, BigUint::from(10_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::CallDataCopy,
        Operation::Push((1, BigUint::from(10_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::Return,
    ];

    let program = Program::from(operations);
    let mut env = Env::default();
    env.tx.data = Bytes::from(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();

    //Test that the memory is correctly copied
    let correct_memory = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    let return_data = result.output().unwrap().as_ref();
    assert_eq!(return_data, correct_memory);
}

#[test]
fn test_calldatacopy_zeros_padding() {
    let operations = vec![
        Operation::Push((1, BigUint::from(10_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::CallDataCopy,
        Operation::Push((1, BigUint::from(10_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::Return,
    ];

    let program = Program::from(operations);
    let mut env = Env::default();
    env.tx.data = Bytes::from(vec![0, 1, 2, 3, 4]);
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();

    //Test that the memory is correctly copied
    let correct_memory = vec![0, 1, 2, 3, 4, 0, 0, 0, 0, 0];
    let return_data = result.output().unwrap().as_ref();
    assert_eq!(return_data, correct_memory);
}

#[test]
fn test_calldatacopy_memory_offset() {
    let operations = vec![
        Operation::Push((1, BigUint::from(5_u8))),
        Operation::Push((1, BigUint::from(1_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::CallDataCopy,
        Operation::Push((1, BigUint::from(5_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::Return,
    ];

    let program = Program::from(operations);
    let mut env = Env::default();
    env.tx.data = Bytes::from(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    env.tx.gas_limit = 1000 + gas_cost::TX_BASE_COST;
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();

    //Test that the memory is correctly copied
    let correct_memory = vec![1, 2, 3, 4, 5];
    let return_data = result.output().unwrap().as_ref();
    assert_eq!(return_data, correct_memory);
}

#[test]
fn test_calldatacopy_calldataoffset() {
    let operations = vec![
        Operation::Push((1, BigUint::from(10_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::Push((1, BigUint::from(1_u8))),
        Operation::CallDataCopy,
        Operation::Push((1, BigUint::from(10_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::Return,
    ];

    let program = Program::from(operations);
    let mut env = Env::default();
    env.tx.data = Bytes::from(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    //Test that the memory is correctly copied
    let correct_memory = vec![0, 0, 1, 2, 3, 4, 5, 6, 7, 8];
    let return_data = result.output().unwrap().as_ref();
    assert_eq!(return_data, correct_memory);
}

#[test]
fn test_calldatacopy_calldataoffset_bigger_than_calldatasize() {
    let operations = vec![
        Operation::Push((1, BigUint::from(10_u8))),
        Operation::Push((1, BigUint::from(30_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::CallDataCopy,
        Operation::Push((1, BigUint::from(10_u8))),
        Operation::Push((1, BigUint::from(0_u8))),
        Operation::Return,
    ];

    let program = Program::from(operations);
    let mut env = Env::default();
    env.tx.data = Bytes::from(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();

    //Test that the memory is correctly copied
    let correct_memory = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let return_data = result.output().unwrap().as_ref();
    assert_eq!(return_data, correct_memory);
}

#[test]
fn log0() {
    let data: [u8; 32] = [0xff; 32];
    let size = 32_u8;
    let memory_offset = 0_u8;
    let program = Program::from(vec![
        // store data in memory
        Operation::Push((32_u8, BigUint::from_bytes_be(&data))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Mstore,
        // execute log0
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Log(0),
    ]);

    let mut env = Env::default();
    env.tx.gas_limit = 999_999;

    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(result.is_success());
    let logs: Vec<LogData> = result.into_logs().into_iter().map(|log| log.data).collect();
    let expected_logs: Vec<LogData> = vec![LogData {
        data: [0xff_u8; 32].into(),
        topics: vec![],
    }];
    assert_eq!(logs, expected_logs);
}

#[test]
fn log1() {
    let data: [u8; 32] = [0xff; 32];
    let size = 32_u8;
    let memory_offset = 0_u8;
    let mut topic: [u8; 32] = [0x00; 32];
    topic[31] = 1;

    let program = Program::from(vec![
        // store data in memory
        Operation::Push((32_u8, BigUint::from_bytes_be(&data))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Mstore,
        // execute log1
        Operation::Push((32_u8, BigUint::from_bytes_be(&topic))),
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Log(1),
    ]);

    let mut env = Env::default();
    env.tx.gas_limit = 999_999;

    let (address, bytecode) = (Address::zero(), Bytecode::from(program.to_bytecode()));
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(result.is_success());
    let logs: Vec<LogData> = result.into_logs().into_iter().map(|log| log.data).collect();
    let expected_logs: Vec<LogData> = vec![LogData {
        data: [0xff_u8; 32].into(),
        topics: vec![U256 { lo: 1, hi: 0 }],
    }];
    assert_eq!(logs, expected_logs);
}

#[test]
fn log2() {
    let data: [u8; 32] = [0xff; 32];
    let size = 32_u8;
    let memory_offset = 0_u8;
    let mut topic1: [u8; 32] = [0x00; 32];
    topic1[31] = 1;
    let mut topic2: [u8; 32] = [0x00; 32];
    topic2[31] = 2;

    let program = Program::from(vec![
        // store data in memory
        Operation::Push((32_u8, BigUint::from_bytes_be(&data))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Mstore,
        // execute log2
        Operation::Push((32_u8, BigUint::from_bytes_be(&topic2))),
        Operation::Push((32_u8, BigUint::from_bytes_be(&topic1))),
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Log(2),
    ]);

    let mut env = Env::default();
    env.tx.gas_limit = 999_999;

    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(result.is_success());
    let logs: Vec<LogData> = result.into_logs().into_iter().map(|log| log.data).collect();
    let expected_logs: Vec<LogData> = vec![LogData {
        data: [0xff_u8; 32].into(),
        topics: vec![U256 { lo: 1, hi: 0 }, U256 { lo: 2, hi: 0 }],
    }];
    assert_eq!(logs, expected_logs);
}

#[test]
fn log3() {
    let data: [u8; 32] = [0xff; 32];
    let size = 32_u8;
    let memory_offset = 0_u8;
    let mut topic1: [u8; 32] = [0x00; 32];
    topic1[31] = 1;
    let mut topic2: [u8; 32] = [0x00; 32];
    topic2[31] = 2;
    let mut topic3: [u8; 32] = [0x00; 32];
    topic3[31] = 3;

    let program = Program::from(vec![
        // store data in memory
        Operation::Push((32_u8, BigUint::from_bytes_be(&data))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Mstore,
        // execute log2
        Operation::Push((32_u8, BigUint::from_bytes_be(&topic3))),
        Operation::Push((32_u8, BigUint::from_bytes_be(&topic2))),
        Operation::Push((32_u8, BigUint::from_bytes_be(&topic1))),
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Log(3),
    ]);

    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(result.is_success());
    let logs: Vec<LogData> = result.into_logs().into_iter().map(|log| log.data).collect();
    let expected_logs: Vec<LogData> = vec![LogData {
        data: [0xff_u8; 32].into(),
        topics: vec![
            U256 { lo: 1, hi: 0 },
            U256 { lo: 2, hi: 0 },
            U256 { lo: 3, hi: 0 },
        ],
    }];
    assert_eq!(logs, expected_logs);
}

#[test]
fn log4() {
    let data: [u8; 32] = [0xff; 32];
    let size = 32_u8;
    let memory_offset = 0_u8;
    let mut topic1: [u8; 32] = [0x00; 32];
    topic1[31] = 1;
    let mut topic2: [u8; 32] = [0x00; 32];
    topic2[31] = 2;
    let mut topic3: [u8; 32] = [0x00; 32];
    topic3[31] = 3;
    let mut topic4: [u8; 32] = [0x00; 32];
    topic4[31] = 4;

    let program = Program::from(vec![
        // store data in memory
        Operation::Push((32_u8, BigUint::from_bytes_be(&data))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Mstore,
        // execute log4
        Operation::Push((32_u8, BigUint::from_bytes_be(&topic4))),
        Operation::Push((32_u8, BigUint::from_bytes_be(&topic3))),
        Operation::Push((32_u8, BigUint::from_bytes_be(&topic2))),
        Operation::Push((32_u8, BigUint::from_bytes_be(&topic1))),
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(memory_offset))),
        Operation::Log(4),
    ]);

    let mut env = Env::default();
    env.tx.gas_limit = 999_999;

    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(result.is_success());
    let logs: Vec<LogData> = result.into_logs().into_iter().map(|log| log.data).collect();
    let expected_logs: Vec<LogData> = vec![LogData {
        data: [0xff_u8; 32].into(),
        topics: vec![
            U256 { lo: 1, hi: 0 },
            U256 { lo: 2, hi: 0 },
            U256 { lo: 3, hi: 0 },
            U256 { lo: 4, hi: 0 },
        ],
    }];
    assert_eq!(logs, expected_logs);
}

#[test]
fn codecopy() {
    let size = 12_u8;
    let offset = 0_u8;
    let dest_offset = 0_u8;
    let program: Program = vec![
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Codecopy,
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Return,
    ]
    .into();

    let mut env = Env::default();
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.clone().to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(&result.is_success());

    let result_data = result.output().unwrap();
    let expected_result = program.to_bytecode();
    assert_eq!(result_data, &expected_result);
}

#[test]
fn codecopy_with_offset_out_of_bounds() {
    // copies to memory the bytecode from the 6th byte (offset = 6)
    // so the result must be [CODECOPY, PUSH, size, PUSH, dest_offset, RETURN, 0, ..., 0]
    let size = 12_u8;
    let offset = 6_u8;
    let dest_offset = 0_u8;
    let program: Program = vec![
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Codecopy, // 6th byte
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Return,
    ]
    .into();

    let mut env = Env::default();
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.clone().to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(&result.is_success());

    let result_data = result.output().unwrap();
    let expected_result = [&program.to_bytecode()[6..], &[0_u8; 6]].concat();
    assert_eq!(result_data, &expected_result);
}

#[test]
fn callvalue_happy_path() {
    let callvalue: u32 = 1500;
    let mut operations = vec![Operation::Callvalue];
    append_return_result_operations(&mut operations);
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.value = EU256::from(callvalue);
    let program = Program::from(operations);
    let bytecode = Bytecode::from(program.to_bytecode());
    let db = Db::new().with_contract(Address::zero(), bytecode);
    let expected_result = BigUint::from(callvalue);
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn callvalue_gas_check() {
    let operations = vec![Operation::Callvalue];
    let needed_gas = gas_cost::CALLVALUE;
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn callvalue_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Callvalue);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn coinbase_happy_path() {
    // taken from evm.codes
    let coinbase_address = "5B38Da6a701c568545dCfcB03FcB875f56beddC4";
    let coinbase: [u8; 20] = hex::decode(coinbase_address)
        .expect("Decoding failed")
        .try_into()
        .expect("Incorrect length");
    let mut operations = vec![Operation::Coinbase];
    append_return_result_operations(&mut operations);
    let (mut env, db) = default_env_and_db_setup(operations);
    env.block.coinbase = coinbase.into();
    let expected_result: [u8; 32] = [&[0u8; 12], &coinbase[..]].concat().try_into().unwrap();
    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn coinbase_gas_check() {
    let operations = vec![Operation::Coinbase];
    let needed_gas = gas_cost::COINBASE;
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn coinbase_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Coinbase);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn timestamp_happy_path() {
    let timestamp: u64 = 1234567890;
    let mut operations = vec![Operation::Timestamp];
    append_return_result_operations(&mut operations);
    let (mut env, db) = default_env_and_db_setup(operations);
    env.block.timestamp = timestamp.into();
    let expected_result = BigUint::from(timestamp);
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn timestamp_gas_check() {
    let operations = vec![Operation::Timestamp];
    let needed_gas = gas_cost::TIMESTAMP;
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn timestamp_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Timestamp);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn basefee() {
    let basefee = 10_u8;
    let mut operations = vec![Operation::Basefee];
    append_return_result_operations(&mut operations);
    let (mut env, db) = default_env_and_db_setup(operations);
    env.block.basefee = EU256::from(basefee);
    let expected_result = BigUint::from(basefee);
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn basefee_gas_check() {
    let program = vec![Operation::Basefee];
    let needed_gas = gas_cost::BASEFEE;
    let env = Env::default();
    run_program_assert_gas_exact(program, env, needed_gas as _);
}

#[test]
fn basefee_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Basefee);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn block_number_check() {
    let mut operations = vec![Operation::Number];
    append_return_result_operations(&mut operations);
    let (mut env, db) = default_env_and_db_setup(operations);
    env.block.number = ethereum_types::U256::from(2147483639);
    let expected_result = BigUint::from(2147483639_u32);
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn block_number_check_gas() {
    let program = vec![Operation::Number];
    let env = Env::default();
    let gas_needed = gas_cost::NUMBER;

    run_program_assert_gas_exact(program, env, gas_needed as _);
}

#[test]
fn block_number_with_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Number);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn sstore_with_stack_underflow() {
    let program = vec![Operation::Push0, Operation::Sstore];
    let (env, db) = default_env_and_db_setup(program);

    run_program_assert_halt(env, db);
}

#[test]
fn sstore_happy_path() {
    let key = 80_u8;
    let value = 11_u8;
    let operations = vec![
        Operation::Push((1_u8, BigUint::from(value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
    ];

    let (env, db) = default_env_and_db_setup(operations);
    let callee_address = env.tx.get_address();
    let mut evm = Evm::new(env, db);

    let res = evm.transact_commit().unwrap();
    assert!(&res.is_success());
    let stored_value = evm.db.read_storage(callee_address, key.into());
    assert_eq!(stored_value, EU256::from(value));
    assert_eq!(stored_value, EU256::from(value));
}

#[test]
fn sstore_sload_happy_path() {
    let key = 80_u8;
    let value = 11_u8;
    let mut operations = vec![
        // sstore
        Operation::Push((1_u8, BigUint::from(value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
        // sload
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sload,
    ];
    append_return_result_operations(&mut operations);
    let (env, db) = default_env_and_db_setup(operations);
    run_program_assert_num_result(env, db, BigUint::from(value));
}

#[test]
fn gasprice_happy_path() {
    let gas_price: u32 = 33192;
    let mut operations = vec![Operation::Gasprice];
    append_return_result_operations(&mut operations);
    let (mut env, db) = default_env_and_db_setup(operations);
    env.tx.gas_price = EU256::from(gas_price);
    let expected_result = BigUint::from(gas_price);
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn gasprice_gas_check() {
    let operations = vec![Operation::Gasprice];
    let needed_gas = gas_cost::GASPRICE;
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn gasprice_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Gasprice);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn chainid_happy_path() {
    let chainid: u64 = 1333;
    let mut operations = vec![Operation::Chainid];
    append_return_result_operations(&mut operations);
    let (mut env, db) = default_env_and_db_setup(operations);
    env.cfg.chain_id = chainid;
    let expected_result = BigUint::from(chainid);
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn chainid_gas_check() {
    let operations = vec![Operation::Chainid];
    let needed_gas = gas_cost::CHAINID;
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn chainid_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Chainid);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn caller_happy_path() {
    let caller = Address::from_str("0x9bbfed6889322e016e0a02ee459d306fc19545d8").unwrap();
    let mut operations = vec![Operation::Caller];
    append_return_result_operations(&mut operations);
    let (mut env, db) = default_env_and_db_setup(operations);
    env.tx.caller = caller;
    let caller_bytes = &caller.to_fixed_bytes();
    //We extend the result to be 32 bytes long.
    let expected_result: [u8; 32] = [&[0u8; 12], &caller_bytes[0..20]]
        .concat()
        .try_into()
        .unwrap();
    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn caller_gas_check() {
    let operations = vec![Operation::Caller];
    let needed_gas = gas_cost::CALLER;
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn caller_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Caller);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn sload_gas_consumption() {
    let program = vec![
        Operation::Push((1_u8, BigUint::from(1_u8))),
        Operation::Sload,
    ];
    let result = gas_cost::PUSHN + gas_cost::SLOAD_COLD;
    let env = Env::default();

    run_program_assert_gas_exact(program, env, result as _);
}

#[test]
fn sload_with_valid_key() {
    let key = 80_u8;
    let value = 11_u8;
    let mut operations = vec![
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sload,
    ];

    append_return_result_operations(&mut operations);

    let (env, db) = default_env_and_db_setup(operations);
    let callee_address = env.tx.get_address();
    let mut evm = Evm::new(env, db);
    evm.db
        .write_storage(callee_address, EU256::from(key), EU256::from(value));
    let result = evm.transact_commit().unwrap();
    assert!(&result.is_success());
    let result = result.output().unwrap().as_ref();
    assert_eq!(EU256::from(result), EU256::from(value));
}

#[test]
fn sload_with_invalid_key() {
    let program = vec![
        Operation::Push((1_u8, BigUint::from(5_u8))),
        Operation::Sload,
    ];
    let (env, db) = default_env_and_db_setup(program);
    let result = BigUint::from(0_u8);
    run_program_assert_num_result(env, db, result);
}

#[test]
fn sload_with_stack_underflow() {
    let program = vec![Operation::Sload];
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn address() {
    let address = Address::from_str("0x9bbfed6889322e016e0a02ee459d306fc19545d8").unwrap();
    let operations = vec![
        Operation::Address,
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ];

    let address_bytes = &address.to_fixed_bytes();
    //We extend the result to be 32 bytes long.
    let expected_result: [u8; 32] = [&[0u8; 12], &address_bytes[0..20]]
        .concat()
        .try_into()
        .unwrap();

    let program = Program::from(operations);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.transact_to = TransactTo::Call(address);

    let db = Db::new().with_contract(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();
    assert!(&result.is_success());
    let result_data = result.output().unwrap().as_ref();
    assert_eq!(result_data, &expected_result);
}

#[test]
fn address_with_gas_cost() {
    let operations = vec![Operation::Address];
    let address = Address::from_low_u64_be(1234);
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(address);
    let needed_gas = gas_cost::ADDRESS;
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn address_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Address);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

// address with more than 20 bytes should be invalid
#[test]
fn balance_with_invalid_address() {
    let a = BigUint::from(1_u8) << 255_u8;
    let balance = EU256::from_dec_str("123456").unwrap();
    let program = Program::from(vec![
        Operation::Push((32_u8, a.clone())),
        Operation::Balance,
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ]);
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;

    let (address, bytecode) = (
        // take the last 20 bytes of the address, because that's what it's done with it's avalid
        Address::from_slice(&a.to_bytes_be()[0..20]),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.caller = address;
    env.tx.transact_to = TransactTo::Call(address);
    let mut db = Db::new().with_contract(address, bytecode);

    db.set_account(address, 0, balance, HashMap::new());

    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(&result.is_success());
    let result = result.output().unwrap();
    let expected_result = BigUint::from(0_u8);
    assert_eq!(BigUint::from_bytes_be(result), expected_result);
}

#[test]
fn balance_with_non_existing_account() {
    let operations = vec![
        Operation::Push((20_u8, BigUint::from(1_u8))),
        Operation::Balance,
    ];
    let (env, db) = default_env_and_db_setup(operations);
    let expected_result = BigUint::from(0_u8);
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn balance_with_existing_account() {
    let address = Address::from_str("0x9bbfed6889322e016e0a02ee459d306fc19545d8").unwrap();
    let balance = EU256::from_dec_str("123456").unwrap();
    let big_a = BigUint::from_bytes_be(address.as_bytes());
    let program = Program::from(vec![
        Operation::Push((20_u8, big_a)),
        Operation::Balance,
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ]);
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;

    let (address, bytecode) = (
        Address::from_str("0x9bbfed6889322e016e0a02ee459d306fc19545d8").unwrap(),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.caller = address;
    env.tx.transact_to = TransactTo::Call(address);
    let mut db = Db::new().with_contract(address, bytecode);

    db.set_account(address, 0, balance, HashMap::new());

    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();

    assert!(&result.is_success());
    let result = result.output().unwrap();
    let expected_result = BigUint::from(123456_u32);
    assert_eq!(BigUint::from_bytes_be(result), expected_result);
}

#[test]
fn balance_with_stack_underflow() {
    let program = vec![Operation::Balance];
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn balance_static_gas_check() {
    let operations = vec![
        Operation::Push((20_u8, BigUint::from(1_u8))),
        Operation::Balance,
    ];
    let env = Env::default();
    let needed_gas = gas_cost::PUSHN + gas_cost::BALANCE;

    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn selfbalance_with_existing_account() {
    let contract_address = Address::from_str("0x9bbfed6889322e016e0a02ee459d306fc19545d8").unwrap();
    let contract_balance: u64 = 12345;
    let mut operations = vec![Operation::SelfBalance];
    append_return_result_operations(&mut operations);
    let program = Program::from(operations);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut db = Db::new().with_contract(contract_address, bytecode);
    db.set_account(contract_address, 0, contract_balance.into(), HashMap::new());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(contract_address);
    env.tx.gas_limit = 999_999;
    let expected_result = BigUint::from(contract_balance);
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn selfbalance_and_balance_with_address_check() {
    let contract_address = Address::from_str("0x9bbfed6889322e016e0a02ee459d306fc19545d8").unwrap();
    let contract_balance: u64 = 12345;
    let mut operations = vec![
        Operation::Address,
        Operation::Balance,
        Operation::SelfBalance,
        Operation::Eq,
    ];
    append_return_result_operations(&mut operations);
    let program = Program::from(operations);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut db = Db::new().with_contract(contract_address, bytecode);
    db.set_account(contract_address, 0, contract_balance.into(), HashMap::new());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(contract_address);
    env.tx.gas_limit = 999_999;
    let expected_result = BigUint::from(1_u8); //True
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn selfbalance_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::SelfBalance);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn selfbalance_gas_check() {
    let operations = vec![Operation::SelfBalance];
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    let needed_gas = gas_cost::SELFBALANCE;

    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn blobbasefee_happy_path() {
    let excess_blob_gas: u64 = 1500;
    let mut operations = vec![Operation::BlobBaseFee];
    append_return_result_operations(&mut operations);
    let (mut env, db) = default_env_and_db_setup(operations);
    env.block.set_blob_base_fee(excess_blob_gas);
    let expected_result = BigUint::from(env.block.blob_gasprice.unwrap());
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn blobbasefee_gas_check() {
    let operations = vec![Operation::BlobBaseFee];
    let needed_gas = gas_cost::BLOBBASEFEE;
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn blobbasefee_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::BlobBaseFee);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn gaslimit_happy_path() {
    let gaslimit: u64 = 300;
    let mut operations = vec![Operation::Gaslimit];
    append_return_result_operations(&mut operations);
    let (mut env, db) = default_env_and_db_setup(operations);
    env.tx.gas_limit = gaslimit + gas_cost::TX_BASE_COST;
    let expected_result = BigUint::from(gaslimit);
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn gaslimit_gas_check() {
    let operations = vec![Operation::Gaslimit];
    let needed_gas = gas_cost::GASLIMIT;
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn gaslimit_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Gaslimit);
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn sstore_gas_cost_on_cold_zero_value() {
    let new_value = 10_u8;

    let used_gas = 22_100 + 2 * gas_cost::PUSHN;
    let needed_gas = used_gas + gas_cost::SSTORE_MIN_REMAINING_GAS;
    let refunded_gas = 0;

    let program = vec![
        Operation::Push((1_u8, BigUint::from(new_value))),
        Operation::Push((1_u8, BigUint::from(80_u8))),
        Operation::Sstore,
    ];
    let (env, db) = default_env_and_db_setup(program);

    run_program_assert_gas_and_refund(env, db, needed_gas as _, used_gas as _, refunded_gas as _);
}

#[test]
fn sstore_gas_cost_on_cold_non_zero_value_to_zero() {
    let new_value: u8 = 0;
    let original_value = 10;

    let used_gas = 5_000 + 2 * gas_cost::PUSHN;
    let needed_gas = used_gas + gas_cost::SSTORE_MIN_REMAINING_GAS;
    let refunded_gas = 4_800.min(used_gas / GAS_REFUND_DENOMINATOR as i64);

    let key = 80_u8;
    let program = vec![
        Operation::Push((1_u8, BigUint::from(new_value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
    ];

    let (env, mut db) = default_env_and_db_setup(program);
    let callee = env.tx.get_address();
    db.write_storage(callee, EU256::from(key), EU256::from(original_value));

    run_program_assert_gas_and_refund(env, db, needed_gas as _, used_gas as _, refunded_gas as _);
}

#[test]
fn sstore_gas_cost_update_warm_value() {
    let new_value: u8 = 20;
    let present_value: u8 = 10;

    let used_gas = 22_200 + 4 * gas_cost::PUSHN;
    let needed_gas = used_gas + gas_cost::SSTORE_MIN_REMAINING_GAS;
    let refunded_gas = 0;

    let key = 80_u8;
    let program = vec![
        // first sstore: gas_cost = 22_100, gas_refund = 0
        Operation::Push((1_u8, BigUint::from(present_value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
        // second sstore: gas_cost = 100, gas_refund = 0
        Operation::Push((1_u8, BigUint::from(new_value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
    ];

    let (env, db) = default_env_and_db_setup(program);

    run_program_assert_gas_and_refund(env, db, needed_gas as _, used_gas as _, refunded_gas as _);
}

#[test]
fn sstore_gas_cost_restore_warm_from_zero() {
    let new_value: u8 = 10;
    let present_value: u8 = 0;
    let original_value: u8 = 10;

    let used_gas = 5_100 + 4 * gas_cost::PUSHN;
    let needed_gas = used_gas + gas_cost::SSTORE_MIN_REMAINING_GAS;
    let refunded_gas = 2_800.min(used_gas / GAS_REFUND_DENOMINATOR as i64);

    let key = 80_u8;
    let program = vec![
        // first sstore: gas_cost = 5_000, gas_refund = 4_800
        Operation::Push((1_u8, BigUint::from(present_value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
        // second sstore: gas_cost = 100, gas_refund = -2_000
        Operation::Push((1_u8, BigUint::from(new_value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
    ];

    let (env, mut db) = default_env_and_db_setup(program);
    let callee = env.tx.get_address();
    db.write_storage(callee, EU256::from(key), EU256::from(original_value));

    run_program_assert_gas_and_refund(env, db, needed_gas as _, used_gas as _, refunded_gas as _);
}

#[test]
fn sstore_gas_cost_update_warm_from_zero() {
    let new_value: u8 = 20;
    let present_value: u8 = 0;
    let original_value: u8 = 10;

    let used_gas = 5_100 + 4 * gas_cost::PUSHN;
    let needed_gas = used_gas + gas_cost::SSTORE_MIN_REMAINING_GAS;
    let refunded_gas = 0;

    let key = 80_u8;
    let program = vec![
        // first sstore: gas_cost = 5_000, gas_refund = 4_800
        Operation::Push((1_u8, BigUint::from(present_value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
        // second sstore: gas_cost = 100, gas_refund = -4_800
        Operation::Push((1_u8, BigUint::from(new_value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
    ];

    let (env, mut db) = default_env_and_db_setup(program);
    let callee = env.tx.get_address();
    db.write_storage(callee, EU256::from(key), EU256::from(original_value));

    run_program_assert_gas_and_refund(env, db, needed_gas as _, used_gas as _, refunded_gas as _);
}

#[test]
fn extcodecopy() {
    // insert the program in the db with address = 100
    // and then copy the program bytecode in memory
    // with extcodecopy(address=100, dest_offset, offset, size)
    let size = 14_u8;
    let offset = 0_u8;
    let dest_offset = 0_u8;
    let address = 100_u8;
    let program: Program = vec![
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Push((1_u8, BigUint::from(address))),
        Operation::ExtcodeCopy,
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Return,
    ]
    .into();

    let mut env = Env::default();
    let (address, bytecode) = (
        Address::from_low_u64_be(address.into()),
        Bytecode::from(program.clone().to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let expected_result = program.to_bytecode();
    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn extcodecopy_with_offset_out_of_bounds() {
    // copies to memory the bytecode from the 8th byte (offset = 8) with size = 12
    // so the result must be [EXTCODECOPY, PUSH, size, PUSH, dest_offset, RETURN, 0,0,0,0,0,0]
    let size = 12_u8;
    let offset = 8_u8;
    let dest_offset = 0_u8;
    let address = 100_u8;
    let program: Program = vec![
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Push((1_u8, BigUint::from(address))),
        Operation::ExtcodeCopy, // 8th byte
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Return,
    ]
    .into();

    let mut env = Env::default();
    let (address, bytecode) = (
        Address::from_low_u64_be(address.into()),
        Bytecode::from(program.clone().to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let expected_result = [&program.to_bytecode()[offset.into()..], &[0_u8; 6]].concat();

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn extcodecopy_with_dirty_memory() {
    // copies to memory the bytecode from the 8th byte (offset = 8) with size = 12
    // so the result must be [EXTCODECOPY, PUSH, size, PUSH, dest_offset, RETURN, 0,0,0,0,0,0]
    // Here we want to test if the copied data overwrites the information already stored in memory
    let size = 10_u8;
    let offset = 43_u8;
    let dest_offset = 2_u8;
    let address = 100_u8;

    let all_ones = BigUint::from_bytes_be(&[0xff_u8; 32]);

    let program: Program = vec![
        //First, we write ones into the memory
        Operation::Push((32_u8, all_ones)),
        Operation::Push0,
        Operation::Mstore,
        //Then, we want make our call to Extcodecopy
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Push((1_u8, BigUint::from(address))),
        Operation::ExtcodeCopy, // 43th byte
        Operation::Push((1_u8, BigUint::from(32_u8))),
        Operation::Push((1_u8, BigUint::from(0_u8))),
        Operation::Return,
    ]
    .into();

    let mut env = Env::default();
    let (address, bytecode) = (
        Address::from_low_u64_be(address.into()),
        Bytecode::from(program.clone().to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let expected_result = [
        &[0xff; 2],                              // 2 bytes of dirty memory (offset = 2)
        &program.to_bytecode()[offset.into()..], // 6 bytes
        &[0_u8; 4],                              // 4 bytes of padding (size = 10 = 6 + 4)
        &[0xff; 20],                             // 20 more bytes of dirty memory
    ]
    .concat();

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn extcodecopy_with_wrong_address() {
    // A wrong address should return an empty bytecode
    let size = 10_u8;
    let offset = 0_u8;
    let dest_offset = 2_u8;
    let address = 100_u8;
    let wrong_address = &[0xff; 32]; // All bits on
    let all_ones = BigUint::from_bytes_be(&[0xff_u8; 32]);

    let program: Program = vec![
        //First, we write ones into the memory
        Operation::Push((32_u8, all_ones)),
        Operation::Push0,
        Operation::Mstore,
        //Begin with Extcodecopy
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Push((32_u8, BigUint::from_bytes_be(wrong_address))),
        Operation::ExtcodeCopy,
        Operation::Push((1_u8, BigUint::from(32_u8))),
        Operation::Push((1_u8, BigUint::from(0_u8))),
        Operation::Return,
    ]
    .into();

    let mut env = Env::default();
    let (address, bytecode) = (
        Address::from_low_u64_be(address.into()),
        Bytecode::from(program.clone().to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let expected_result = [
        vec![0xff; 2],  // 2 bytes of dirty memory (offset = 2)
        vec![0_u8; 10], // 4 bytes of padding (size = 10)
        vec![0xff; 20], // 20 more bytes of dirty memory
    ]
    .concat();

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn prevrandao() {
    let mut program = vec![Operation::Prevrandao];
    append_return_result_operations(&mut program);
    let (mut env, db) = default_env_and_db_setup(program);
    let randao_str = "0xce124dee50136f3f93f19667fb4198c6b94eecbacfa300469e5280012757be94";
    let randao = B256::from_str(randao_str).expect("Error while converting str to B256");
    env.block.prevrandao = Some(randao);

    let expected_result = randao.as_bytes();
    run_program_assert_bytes_result(env, db, expected_result);
}

#[test]
fn prevrandao_check_gas() {
    let program = vec![Operation::Prevrandao];
    let env = Env::default();
    let gas_needed = gas_cost::PREVRANDAO;

    run_program_assert_gas_exact(program, env, gas_needed as _);
}

#[test]
fn prevrandao_with_stack_overflow() {
    let mut program = vec![Operation::Push0; 1024];
    program.push(Operation::Prevrandao);
    let (env, db) = default_env_and_db_setup(program);

    run_program_assert_halt(env, db);
}

#[test]
fn prevrandao_when_randao_is_not_set() {
    let program = vec![Operation::Prevrandao];
    let (env, db) = default_env_and_db_setup(program);
    let expected_result = 0_u8;
    run_program_assert_num_result(env, db, expected_result.into());
}

#[test]
fn extcodesize() {
    let address = 40_u8;
    let mut operations = vec![
        Operation::Push((1_u8, address.into())),
        Operation::ExtcodeSize,
    ];
    append_return_result_operations(&mut operations);

    let mut env = Env::default();
    let program = Program::from(operations);
    let (address, bytecode) = (
        Address::from_low_u64_be(address as _),
        Bytecode::from(program.clone().to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_contract(address, bytecode);
    let expected_result = program.to_bytecode().len();
    run_program_assert_num_result(env, db, expected_result.into())
}

#[test]
fn extcodesize_with_stack_underflow() {
    let program = vec![Operation::ExtcodeSize];
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn extcodesize_gas_check() {
    // in this case we are not considering cold and warm accesses
    // we assume every access is warm
    let address = 40_u8;
    let operations = vec![
        Operation::Push((1_u8, address.into())),
        Operation::ExtcodeSize,
    ];
    let needed_gas = gas_cost::PUSHN + gas_cost::EXTCODESIZE_WARM;
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn extcodesize_with_wrong_address() {
    let address = 0_u8;
    let operations = vec![
        Operation::Push((1_u8, address.into())),
        Operation::ExtcodeSize,
    ];
    let (env, db) = default_env_and_db_setup(operations);
    let expected_result = 0_u8;
    run_program_assert_num_result(env, db, expected_result.into())
}

#[test]
fn extcodesize_with_invalid_address() {
    // Address with upper 12 bytes filled with 1s is invalid
    let address = BigUint::from_bytes_be(&[0xff; 32]);
    let operations = vec![Operation::Push((32_u8, address)), Operation::ExtcodeSize];
    let (env, db) = default_env_and_db_setup(operations);
    let expected_result = 0_u8;
    run_program_assert_num_result(env, db, expected_result.into())
}

#[test]
fn blobhash() {
    // set 2 blobhashes in env.tx.blob_hashes and retrieve the one at index 1.
    let index = 1_u8;
    let mut program = vec![Operation::Push((1_u8, index.into())), Operation::BlobHash];
    append_return_result_operations(&mut program);
    let (mut env, db) = default_env_and_db_setup(program);
    let blobhash_str = "0xce124dee50136f3f93f19667fb4198c6b94eecbacfa300469e5280012757be94";
    let blobhash = B256::from_str(blobhash_str).expect("Error while converting str to B256");
    env.tx.blob_hashes = vec![B256::default(), blobhash];

    let expected_result = blobhash.to_fixed_bytes();
    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn blobhash_check_gas() {
    let program = vec![Operation::Push((1_u8, 0_u8.into())), Operation::BlobHash];
    let mut env = Env::default();
    env.tx.blob_hashes = vec![B256::default()];
    let gas_needed = gas_cost::PUSHN + gas_cost::BLOBHASH;

    run_program_assert_gas_exact(program, env, gas_needed as _);
}

#[test]
fn blobhash_with_stack_underflow() {
    let program = vec![Operation::BlobHash];
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn blobhash_with_index_out_of_bounds() {
    // when index >= len(blob_hashes) the result must be a 32-byte-zero.
    let index = 2_u8;
    let mut program = vec![Operation::Push((1_u8, index.into())), Operation::BlobHash];
    append_return_result_operations(&mut program);
    let (env, db) = default_env_and_db_setup(program);

    let expected_result = [0x00; 32];
    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn blobhash_with_index_too_big() {
    // when index > usize::MAX the result must be a 32-byte-zero.
    let index: u128 = usize::MAX as u128 + 1;
    let mut program = vec![Operation::Push((32_u8, index.into())), Operation::BlobHash];
    append_return_result_operations(&mut program);
    let (env, db) = default_env_and_db_setup(program);

    let expected_result = [0x00; 32];
    run_program_assert_bytes_result(env, db, &expected_result);
}

#[rstest]
#[case(Operation::Call)]
#[case(Operation::StaticCall)]
fn call_simple_callee_call(#[case] call_type: Operation) {
    let (a, b) = (BigUint::from(3_u8), BigUint::from(5_u8));
    let db = Db::new();

    // Callee
    let mut callee_ops = vec![
        Operation::Push0,
        Operation::CalldataLoad,
        Operation::Push((1_u8, BigUint::from(32_u8))),
        Operation::CalldataLoad,
        Operation::Add,
    ];
    append_return_result_operations(&mut callee_ops);

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = db.with_contract(callee_address, bytecode);

    let gas = 100_u8;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 64_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

    let caller_address = Address::from_low_u64_be(4040);

    //Add or not the value argument
    let value_op_vec = match call_type {
        Operation::Call | Operation::CallCode => {
            vec![Operation::Push((1_u8, BigUint::from(value)))]
        }
        Operation::StaticCall | Operation::DelegateCall => vec![],
        _ => panic!("Only call opcodes allowed"),
    };

    let call_op_vec = match call_type {
        Operation::Call => vec![Operation::Call],
        Operation::StaticCall => vec![Operation::StaticCall],
        Operation::CallCode => vec![Operation::CallCode],
        Operation::DelegateCall => vec![Operation::DelegateCall],
        _ => panic!("Only call opcodes allowed"),
    };

    let caller_ops = [
        vec![
            Operation::Push((32_u8, b.clone())),                 //Operand B
            Operation::Push0,                                    //
            Operation::Mstore,                                   //Store in mem address 0
            Operation::Push((32_u8, a.clone())),                 //Operand A
            Operation::Push((1_u8, BigUint::from(32_u8))),       //
            Operation::Mstore,                                   //Store in mem address 32
            Operation::Push((1_u8, BigUint::from(ret_size))),    //Ret size
            Operation::Push((1_u8, BigUint::from(ret_offset))),  //Ret offset
            Operation::Push((1_u8, BigUint::from(args_size))),   //Args size
            Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        ],
        value_op_vec,
        vec![
            Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
            Operation::Push((1_u8, BigUint::from(gas))),                                 //Gas
        ],
        call_op_vec,
        vec![
            //This ops will return the value stored in memory, the call status and the caller balance
            //call status
            Operation::Push((1_u8, 32_u8.into())),
            Operation::Mstore,
            //Return
            Operation::Push((1_u8, 64_u8.into())),
            Operation::Push0,
            Operation::Return,
        ],
    ]
    .concat();

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let db = db.with_contract(caller_address, bytecode);

    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());

    let res_bytes: &[u8] = result.output().unwrap();

    let expected_contract_data_result = a + b;
    let expected_contract_status_result = 1_u8.into();

    let contract_data_result = BigUint::from_bytes_be(&res_bytes[..32]);
    let contract_status_result = BigUint::from_bytes_be(&res_bytes[32..]);

    assert_eq!(contract_status_result, expected_contract_status_result);
    assert_eq!(contract_data_result, expected_contract_data_result);
}

#[rstest]
#[case(Operation::Call)]
#[case(Operation::CallCode)]
fn call_addition_with_value_transfer(#[case] call_type: Operation) {
    let (a, b) = (BigUint::from(3_u8), BigUint::from(5_u8));
    let db = Db::new();

    // Callee
    let callee_balance = 0_u8;
    let mut callee_ops = vec![
        Operation::Push0,
        Operation::CalldataLoad,
        Operation::Push((1_u8, BigUint::from(32_u8))),
        Operation::CalldataLoad,
        Operation::Add,
    ];
    append_return_result_operations(&mut callee_ops);

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let mut db = db.with_contract(callee_address, bytecode);
    db.set_account(callee_address, 0, callee_balance.into(), Default::default());

    let gas = 100_u8;
    let value = 1_u8;
    let args_offset = 0_u8;
    let args_size = 64_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

    let caller_address = Address::from_low_u64_be(4040);
    let call_op_vec = match call_type {
        Operation::Call => vec![Operation::Call],
        Operation::CallCode => vec![Operation::CallCode],
        _ => panic!("Only Call and CallCode allowed on this test"),
    };
    let caller_ops = [
        vec![
            Operation::Push((32_u8, b.clone())),                 //Operand B
            Operation::Push0,                                    //
            Operation::Mstore,                                   //Store in mem address 0
            Operation::Push((32_u8, a.clone())),                 //Operand A
            Operation::Push((1_u8, BigUint::from(32_u8))),       //
            Operation::Mstore,                                   //Store in mem address 32
            Operation::Push((1_u8, BigUint::from(ret_size))),    //Ret size
            Operation::Push((1_u8, BigUint::from(ret_offset))),  //Ret offset
            Operation::Push((1_u8, BigUint::from(args_size))),   //Args size
            Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
            Operation::Push((1_u8, BigUint::from(value))),       //Value
            Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
            Operation::Push((1_u8, BigUint::from(gas))),         //Gas
        ],
        call_op_vec,
        vec![
            Operation::Push((1_u8, 32_u8.into())),
            Operation::Mstore,
            //Return
            Operation::Push((1_u8, 64_u8.into())),
            Operation::Push0,
            Operation::Return,
        ],
    ]
    .concat();

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let caller_balance = 100_u8;
    let mut db = db.with_contract(caller_address, bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());

    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());

    let res_bytes: &[u8] = result.output().unwrap();

    let expected_contract_data_result = a + b;
    let expected_caller_balance_result = (caller_balance - value).into();
    let expected_callee_balance_result = (callee_balance + value).into();
    let expected_contract_status_result = 1_u8.into();

    let contract_data_result = BigUint::from_bytes_be(&res_bytes[..32]);
    let contract_status_result = BigUint::from_bytes_be(&res_bytes[32..]);
    let final_caller_balance = evm
        .db
        .basic(caller_address)
        .unwrap()
        .unwrap_or_default()
        .balance;
    let final_callee_balance = evm
        .db
        .basic(callee_address)
        .unwrap()
        .unwrap_or_default()
        .balance;

    assert_eq!(contract_status_result, expected_contract_status_result);
    assert_eq!(contract_data_result, expected_contract_data_result);
    assert_eq!(final_caller_balance, expected_caller_balance_result);
    assert_eq!(final_callee_balance, expected_callee_balance_result);
}

#[rstest]
#[case(Operation::Call)]
#[case(Operation::CallCode)]
fn call_without_enough_balance(#[case] call_type: Operation) {
    let db = Db::new();

    // Callee
    let callee_balance = 0;
    let mut callee_ops = vec![Operation::Push0];
    append_return_result_operations(&mut callee_ops);

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let mut db = db.with_contract(callee_address, bytecode);
    db.set_account(callee_address, 0, callee_balance.into(), Default::default());

    let gas = 100_u8;
    let value = 1_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

    let caller_address = Address::from_low_u64_be(4040);
    let call_op_vec = match call_type {
        Operation::Call => vec![Operation::Call],
        Operation::CallCode => vec![Operation::CallCode],
        _ => panic!("Only Call and CallCode allowed on this test"),
    };
    let mut caller_ops = [
        vec![
            Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
            Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
            Operation::Push((1_u8, BigUint::from(args_size))), //Args size
            Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
            Operation::Push((1_u8, BigUint::from(value))),    //Value
            Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
            Operation::Push((1_u8, BigUint::from(gas))),      //Gas
        ],
        call_op_vec,
    ]
    .concat();
    append_return_result_operations(&mut caller_ops);

    let caller_balance: u8 = 0;
    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let mut db = db.with_contract(caller_address, bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());

    let expected_contract_call_result = 0_u8.into(); //Call failed
    let expected_caller_balance_result = caller_balance.into();
    let expected_callee_balance_result = callee_balance.into();

    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();
    let result_data = BigUint::from_bytes_be(result.output().unwrap_or(&Bytes::new()));
    let final_caller_balance = evm
        .db
        .basic(caller_address)
        .unwrap()
        .unwrap_or_default()
        .balance;
    let final_callee_balance = evm
        .db
        .basic(callee_address)
        .unwrap()
        .unwrap_or_default()
        .balance;

    assert!(result.is_success());
    assert_eq!(result_data, expected_contract_call_result);
    assert_eq!(final_caller_balance, expected_caller_balance_result);
    assert_eq!(final_callee_balance, expected_callee_balance_result);
}

#[rstest]
#[case(Operation::Call)]
#[case(Operation::StaticCall)]
#[case(Operation::CallCode)]
#[case(Operation::DelegateCall)]
fn call_gas_check_with_value_zero_args_return_and_non_empty_callee(#[case] call_type: Operation) {
    /*
    This will test the gas consumption for a call with the following conditions:
    Value: 0
    Argument size: 64 bytes
    Argument offset: 0 bytes
    Return size: 32 bytes
    Return offset: 0 bytes
    Called account empty?: False
    Address access is warm?: True
    */
    let db = Db::new();

    // Callee
    let callee_ops = vec![
        Operation::Push0,
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ];

    let callee_gas_cost = gas_cost::PUSHN
        + gas_cost::PUSH0 * 3
        + gas_cost::MSTORE
        + gas_cost::memory_expansion_cost(0, 32);

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = db.with_contract(callee_address, bytecode);

    let gas = callee_gas_cost as u8;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 64_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

    let caller_address = Address::from_low_u64_be(4040);

    //Add or not the value argument
    let nargs = match call_type {
        Operation::Call | Operation::CallCode => 7,
        Operation::StaticCall | Operation::DelegateCall => 6,
        _ => panic!("Only call related opcodes allowd"),
    };

    //Add or not the value argument
    let value_op_vec = match call_type {
        Operation::Call | Operation::CallCode => {
            vec![Operation::Push((1_u8, BigUint::from(value)))]
        }
        Operation::StaticCall | Operation::DelegateCall => vec![],
        _ => panic!("Only call related opcodes allowd"),
    };

    let caller_ops = [
        vec![
            Operation::Push((32_u8, BigUint::default())), //Operand B
            Operation::Push0,                             //
            Operation::Mstore,                            //Store in mem address 0
            Operation::Push((32_u8, BigUint::default())), //Operand A
            Operation::Push((1_u8, BigUint::from(32_u8))), //
            Operation::Mstore,                            //Store in mem address 32
            Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
            Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
            Operation::Push((1_u8, BigUint::from(args_size))), //Args size
            Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        ],
        value_op_vec,
        vec![
            Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
            Operation::Push((1_u8, BigUint::from(gas))),                                 //Gas
        ],
        vec![call_type],
    ]
    .concat();

    let caller_gas_cost = gas_cost::PUSHN * (3 + nargs)
        + gas_cost::PUSH0
        + gas_cost::MSTORE * 2
        + call_opcode::WARM_MEMORY_ACCESS_COST as i64
        + gas_cost::memory_expansion_cost(0, 64);

    let available_gas = 1e6;
    let needed_gas = caller_gas_cost + callee_gas_cost;
    let refund_gas = 0;

    let caller_balance: u8 = 0;
    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let mut db = db.with_contract(caller_address, bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());

    run_program_assert_gas_and_refund(
        env,
        db,
        available_gas as _,
        needed_gas as _,
        refund_gas as _,
    );
}

#[rstest]
// Case with offset=0; size=0
#[case(
    0,
    0,
    // Final memory state
    //ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff
    &[0xFF; 32])]
// Case with offset=1; size=3
#[case(
    1,
    3,
    // Final memory state
    //ff333333ffffffffffffffffffffffffffffffffffffffffffffffffffffffff
    &[
    vec![0xFF_u8; 1],
    vec![0x33_u8; 3],
    vec![0xFF_u8; 28]
].concat())]
// Case with offset=3; size=4
#[case(
    3,
    4,
    //Final memory state
    //0xffffff33333333ffffffffffffffffffffffffffffffffffffffffffffffffff
    &[
    vec![0xFF_u8; 3],
    vec![0x33_u8; 4],
    vec![0xFF_u8; 25]
].concat())]
fn call_return_with_offset_and_size(
    #[case] offset: u8,
    #[case] size: u8,
    #[case] expected_result: &[u8],
) {
    let db = Db::new();
    let return_data = [0x33_u8; 5];

    // Callee
    let callee_ops = vec![
        Operation::Push((5_u8, BigUint::from_bytes_be(&return_data))),
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1_u8, 5_u8.into())),
        Operation::Push((1_u8, 27_u8.into())),
        Operation::Return,
    ];

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = db.with_contract(callee_address, bytecode);

    let gas = 100_u8;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;

    let initial_memory_state = [0xFF_u8; 32]; //All 1s

    let caller_address = Address::from_low_u64_be(4040);
    let caller_ops = vec![
        // Set up memory with all 1s
        Operation::Push((32_u8, BigUint::from_bytes_be(&initial_memory_state))),
        Operation::Push0,
        Operation::Mstore,
        // Make the Call
        Operation::Push((1_u8, BigUint::from(size))), //Ret size
        Operation::Push((1_u8, BigUint::from(offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((1_u8, BigUint::from(value))), //Value
        Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((1_u8, BigUint::from(gas))),  //Gas
        Operation::Call,
        // Return 32 bytes of data
        Operation::Push((1_u8, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let db = db.with_contract(caller_address, bytecode);

    run_program_assert_bytes_result(env, db, expected_result);
}

#[rstest]
#[case(Operation::Call)]
#[case(Operation::StaticCall)]
#[case(Operation::CallCode)]
#[case(Operation::DelegateCall)]
fn call_check_stack_underflow(#[case] call_type: Operation) {
    let program = vec![call_type];
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[rstest]
#[case(Operation::Call)]
#[case(Operation::CallCode)]
fn call_gas_check_with_value_and_empty_account(#[case] call_type: Operation) {
    /*
    This will test the gas consumption for a call with the following conditions:
    Value: 3
    Argument size: 0 bytes
    Argument offset: 0 bytes
    Return size: 0 bytes
    Return offset: 0 bytes
    Called account empty?: True
    Address access is warm?: True
    */
    let db = Db::new();

    // Callee
    let (callee_address, bytecode) = (Address::from_low_u64_be(8080), Bytecode::default());
    let mut db = db.with_contract(callee_address, bytecode);
    db.set_account(callee_address, 0, EU256::zero(), Default::default());

    // Caller
    let caller_address = Address::from_low_u64_be(4040);
    let gas = 255_u8;
    let value = 3_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

    let call_op_vec = match call_type {
        Operation::Call => vec![Operation::Call],
        Operation::CallCode => vec![Operation::CallCode],
        _ => panic!("Only Call and CallCode allowed on this test"),
    };
    let caller_ops = [
        vec![
            Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
            Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
            Operation::Push((1_u8, BigUint::from(args_size))), //Args size
            Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
            Operation::Push((1_u8, BigUint::from(value))),    //Value
            Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
            Operation::Push((1_u8, BigUint::from(gas))),      //Gas
        ],
        call_op_vec,
    ]
    .concat();

    //address_access_cost + positive_value_cost + value_to_empty_account_cost
    let caller_call_cost = call_opcode::WARM_MEMORY_ACCESS_COST
        + call_opcode::NOT_ZERO_VALUE_COST
        + call_opcode::EMPTY_CALLEE_COST;
    let needed_gas = gas_cost::PUSHN * 7 + caller_call_cost as i64;

    let caller_balance: u8 = 5;
    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let mut db = db.with_contract(caller_address, bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());

    run_program_assert_gas_exact_with_db(env, db, needed_gas as _);
}

#[rstest]
#[case(Operation::Call)]
#[case(Operation::StaticCall)]
#[case(Operation::CallCode)]
#[case(Operation::DelegateCall)]
fn call_callee_returns_new_value(#[case] call_type: Operation) {
    let db = Db::new();
    let origin = Address::from_low_u64_be(79);
    let origin_value = 5_u8;

    // Callee
    let mut callee_ops = vec![Operation::Callvalue];
    append_return_result_operations(&mut callee_ops);

    // Caller
    let gas = 100_000_000_u32;
    let args_offset = 0_u8;
    let value = 3_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

    let program = Program::from(callee_ops);
    let (callee_address, callee_bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = db.with_contract(callee_address, callee_bytecode);

    let caller_address = Address::from_low_u64_be(4040);

    let value_op_vec = match call_type {
        Operation::Call | Operation::CallCode => {
            vec![Operation::Push((1_u8, BigUint::from(value)))]
        }
        Operation::StaticCall | Operation::DelegateCall => vec![],
        _ => panic!("Only call related opcodes allowed"),
    };

    let caller_ops = [
        vec![
            Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
            Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
            Operation::Push((1_u8, BigUint::from(args_size))), //Args size
            Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        ],
        value_op_vec,
        vec![
            Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
            Operation::Push((32_u8, BigUint::from(gas))),                                //Gas
        ],
        vec![call_type.clone()],
        vec![
            Operation::Push((1_u8, 32_u8.into())),
            Operation::Push0,
            Operation::Return,
        ],
    ]
    .concat();

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let caller_balance = 100_u8;
    let mut env = Env::default();
    let mut db = db.with_contract(caller_address, caller_bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = origin;
    env.tx.value = origin_value.into();

    let expected_result = match call_type {
        Operation::StaticCall => 0,
        Operation::DelegateCall => origin_value,
        _ => value,
    };

    run_program_assert_num_result(env, db, expected_result.into());
}

#[rstest]
#[case(Operation::Call)]
#[case(Operation::CallCode)]
#[case(Operation::StaticCall)]
#[case(Operation::DelegateCall)]
fn call_callee_returns_caller(#[case] call_type: Operation) {
    let db = Db::new();
    let origin = Address::from_low_u64_be(79);

    // Callee
    let mut callee_ops = vec![Operation::Caller];
    append_return_result_operations(&mut callee_ops);

    // Caller
    let gas = 100_000_000_u32;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

    let program = Program::from(callee_ops);
    let (callee_address, callee_bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = db.with_contract(callee_address, callee_bytecode);
    let caller_address = Address::from_low_u64_be(4040);
    let value_op_vec = match call_type {
        Operation::Call | Operation::CallCode => {
            vec![Operation::Push((1_u8, BigUint::from(value)))]
        }
        Operation::StaticCall | Operation::DelegateCall => vec![],
        _ => panic!("Only call opcodes allowed"),
    };
    let caller_ops = [
        vec![
            Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
            Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
            Operation::Push((1_u8, BigUint::from(args_size))), //Args size
            Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        ],
        value_op_vec,
        vec![
            Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
            Operation::Push((32_u8, BigUint::from(gas))),                                //Gas
        ],
        vec![call_type.clone()],
        vec![
            Operation::Push((1_u8, 20_u8.into())),
            Operation::Push((1_u8, 12_u8.into())),
            Operation::Return,
        ],
    ]
    .concat();

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = db.with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = origin;

    let expected_result = match call_type {
        Operation::DelegateCall => origin,
        _ => caller_address,
    };

    run_program_assert_bytes_result(env, db, expected_result.as_fixed_bytes());
}

#[ignore] //This should be run when storage fix on CALL is made
#[test]
fn call_callee_storage_modified() {
    let db = Db::new();
    let origin = Address::from_low_u64_be(79);
    let origin_value = 1_u8;

    // Callee
    let key = 80_u8;
    let value = 11_u8;
    let callee_ops = vec![
        Operation::Push((1_u8, BigUint::from(value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
    ];

    // Caller
    let sent_gas = 100_000_000_u32;
    let args_offset = 0_u8;
    let sent_value = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

    let program = Program::from(callee_ops);
    let (callee_address, callee_bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = db.with_contract(callee_address, callee_bytecode);

    let caller_address = Address::from_low_u64_be(4040);
    let caller_ops = vec![
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((1_u8, BigUint::from(sent_value))), //Value
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, BigUint::from(sent_gas))), //Gas
        Operation::Call,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = db.with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = origin;
    env.tx.value = origin_value.into();

    let mut evm = Evm::new(env, db);
    let res = evm.transact_commit().unwrap();
    assert!(res.is_success());

    let stored_value = evm.db.read_storage(callee_address, key.into());
    assert_eq!(stored_value, EU256::from(value));
}

#[test]
fn staticcall_on_precompile_ecrecover_happy_path() {
    let gas = 100_000_000_u32;
    let args_offset = 0_u8;
    let args_size = 128_u8;
    let ret_offset = 128_u8;
    let ret_size = 32_u8;
    let hash =
        hex::decode("456e9aea5e197a1f1af7a3e85a3212fa4049a3ba34c2289b4c860fc0b0c64ef3").unwrap();
    let v: u8 = 28;
    let r =
        hex::decode("9242685bf161793cc25603c231bc2f568eb630ea16aa137d2664ac8038825608").unwrap();
    let s =
        hex::decode("4f8ae3bd7535248d0bd448298cc2e2071e56992d0774dc340c368ae950852ada").unwrap();
    let callee_address = Address::from_low_u64_be(ECRECOVER_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let expected_result =
        hex::decode("0000000000000000000000007156526fbd7a3c72969b54f64e42c10fbb768c8a").unwrap();

    let caller_ops = vec![
        // Place the parameters in memory
        Operation::Push((32_u8, BigUint::from_bytes_be(&hash))),
        Operation::Push((1_u8, BigUint::ZERO)),
        Operation::Mstore,
        Operation::Push((1_u8, BigUint::from(v))),
        Operation::Push((1_u8, BigUint::from(0x20_u8))),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(&r))),
        Operation::Push((1_u8, BigUint::from(0x40_u8))),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(&s))),
        Operation::Push((1_u8, BigUint::from(0x60_u8))),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, BigUint::from(gas))),     //Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecrecover_without_gas() {
    let gas = 0_u32;
    let args_offset = 0_u8;
    let args_size = 128_u8;
    let ret_offset = 128_u8;
    let ret_size = 32_u8;
    let hash =
        hex::decode("456e9aea5e197a1f1af7a3e85a3212fa4049a3ba34c2289b4c860fc0b0c64ef3").unwrap();
    let v: u8 = 28;
    let r =
        hex::decode("9242685bf161793cc25603c231bc2f568eb630ea16aa137d2664ac8038825608").unwrap();
    let s =
        hex::decode("4f8ae3bd7535248d0bd448298cc2e2071e56992d0774dc340c368ae950852ada").unwrap();
    let callee_address = Address::from_low_u64_be(ECRECOVER_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let expected_result = [0_u8; 32];

    let caller_ops = vec![
        // Place the parameters in memory
        Operation::Push((32_u8, BigUint::from_bytes_be(&hash))),
        Operation::Push((1_u8, BigUint::ZERO)),
        Operation::Mstore,
        Operation::Push((1_u8, BigUint::from(v))),
        Operation::Push((1_u8, BigUint::from(0x20_u8))),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(&r))),
        Operation::Push((1_u8, BigUint::from(0x40_u8))),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(&s))),
        Operation::Push((1_u8, BigUint::from(0x60_u8))),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, BigUint::from(gas))),     //Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_identity_happy_path() {
    let gas: u32 = 100_000_000;
    let args_offset: u8 = 31;
    let args_size: u8 = 1;
    let ret_offset: u8 = 63;
    let ret_size: u8 = 1;
    let data: u8 = 0xff;
    let callee_address = Address::from_low_u64_be(IDENTITY_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let expected_result = [0xff];

    let caller_ops = vec![
        // Place the parameter in memory
        Operation::Push((1_u8, BigUint::from(data))),
        Operation::Push((1_u8, BigUint::ZERO)),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, BigUint::from(gas))),     //Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_sha2_256_happy_path() {
    let gas: u32 = 100_000_000;
    let args_offset: u8 = 31;
    let args_size: u8 = 1;
    let ret_offset: u8 = 32;
    let ret_size: u8 = 32;
    let data: u8 = 0xff;
    let callee_address = Address::from_low_u64_be(SHA2_256_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let expected_result =
        hex::decode("a8100ae6aa1940d0b663bb31cd466142ebbdbd5187131b92d93818987832eb89").unwrap();

    let mut caller_ops = vec![
        // Place the parameter in memory
        Operation::Push((1_u8, BigUint::from(data))),
        Operation::Push((1_u8, BigUint::ZERO)),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, BigUint::from(gas))),     //Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, 32_u8.into())),
        Operation::Push((1_u8, 32_u8.into())),
        Operation::Return,
    ];

    append_return_result_operations(&mut caller_ops);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ripemd_160_happy_path() {
    let gas: u32 = 100_000_000;
    let args_offset: u8 = 31;
    let args_size: u8 = 1;
    let ret_offset: u8 = 0;
    let ret_size: u8 = 32;
    let data: u8 = 0xff;
    let callee_address = Address::from_low_u64_be(RIPEMD_160_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let expected_result = hex::decode("2c0c45d3ecab80fe060e5f1d7057cd2f8de5e557").unwrap();

    let mut caller_ops = vec![
        // Place the parameter in memory
        Operation::Push((1_u8, BigUint::from(data))),
        Operation::Push((1_u8, BigUint::ZERO)),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, BigUint::from(gas))),     //Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, 20_u8.into())),
        Operation::Push((1_u8, 12_u8.into())),
        Operation::Return,
    ];

    append_return_result_operations(&mut caller_ops);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_modexp_happy_path() {
    let ret_size: u8 = 2;
    let ret_offset: u8 = 159;
    let args_size: u8 = 100; // bsize (32) + esize (32) + msize (32) + b (1) + e (1) + m (2). In total, 100 bytes
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(MODEXP_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let b_size: u8 = 1;
    let e_size: u8 = 1;
    let m_size: u8 = 2;
    // Word with b = 8, e = 9, m = 501
    let params =
        &hex::decode("080901F500000000000000000000000000000000000000000000000000000000").unwrap();

    // 329 = (8 ^ 9) mod 501
    let expected_result = 329_u16.to_be_bytes();

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((1_u8, b_size.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((1_u8, e_size.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((1_u8, m_size.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(params))),
        Operation::Push((1_u8, 0x60_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), //Ret size
        Operation::Push((1_u8, ret_offset.into())), //Ret offset
        Operation::Push((1_u8, args_size.into())), //Args size
        Operation::Push((1_u8, args_offset.into())), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, gas.into())),     //Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecadd_happy_path() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 128;
    let args_size: u8 = 128;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECADD_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 2;
    let x2: u8 = 1;
    let y2: u8 = 2;

    let expected_x =
        hex::decode("030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd3").unwrap();
    let expected_y =
        hex::decode("15ed738c0e0a7c92e7845f96b2ae9c0a68a6a449e3538fc7ff3ebf7a5a18a2c4").unwrap();
    let expected_result = [expected_x, expected_y].concat();

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, x2.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y2.into())),
        Operation::Push((1_u8, 0x60_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), //Ret size
        Operation::Push((1_u8, ret_offset.into())), //Ret offset
        Operation::Push((1_u8, args_size.into())), //Args size
        Operation::Push((1_u8, args_offset.into())), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, gas.into())),     //Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecadd_infinity_with_valid_point() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 128;
    let args_size: u8 = 128;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECADD_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 0;
    let y1: u8 = 0;
    let x2: u8 = 1;
    let y2: u8 = 2;

    let expected_x =
        hex::decode("0000000000000000000000000000000000000000000000000000000000000001").unwrap();
    let expected_y =
        hex::decode("0000000000000000000000000000000000000000000000000000000000000002").unwrap();
    let expected_result = [expected_x, expected_y].concat();

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, x2.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y2.into())),
        Operation::Push((1_u8, 0x60_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecadd_valid_point_with_infinity() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 128;
    let args_size: u8 = 128;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECADD_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 2;
    let x2: u8 = 0;
    let y2: u8 = 0;

    let expected_x =
        hex::decode("0000000000000000000000000000000000000000000000000000000000000001").unwrap();
    let expected_y =
        hex::decode("0000000000000000000000000000000000000000000000000000000000000002").unwrap();
    let expected_result = [expected_x, expected_y].concat();

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, x2.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y2.into())),
        Operation::Push((1_u8, 0x60_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecadd_infinity_twice() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 128;
    let args_size: u8 = 128;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECADD_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 0;
    let y1: u8 = 0;
    let x2: u8 = 0;
    let y2: u8 = 0;

    let expected_result = Bytes::from([0u8; 64].to_vec());

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, x2.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y2.into())),
        Operation::Push((1_u8, 0x60_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecadd_with_invalid_first_point() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 128;
    let args_size: u8 = 128;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000;
    let callee_address = Address::from_low_u64_be(ECADD_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 1;
    let x2: u8 = 1;
    let y2: u8 = 2;

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, x2.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y2.into())),
        Operation::Push((1_u8, 0x60_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_num_result(env, db, BigUint::ZERO);
}

#[test]
fn staticcall_on_precompile_ecadd_with_invalid_second_point() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 128;
    let args_size: u8 = 128;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000;
    let callee_address = Address::from_low_u64_be(ECADD_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 2;
    let x2: u8 = 1;
    let y2: u8 = 1;

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, x2.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y2.into())),
        Operation::Push((1_u8, 0x60_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_num_result(env, db, BigUint::ZERO);
}

#[test]
fn staticcall_on_precompile_ecadd_with_missing_stack_parameter() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 128;
    let args_size: u8 = 128;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000;
    let callee_address = Address::from_low_u64_be(ECADD_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 2;
    let x2: u8 = 1;
    let y2: u8 = 2;

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, x2.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y2.into())),
        Operation::Push((1_u8, 0x60_u8.into())),
        Operation::Mstore,
        // Do the call
        // Operation::Push((1_u8, ret_size.into())), // Ret size missing
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())),  // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),       // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_halt(env, db);
}

#[test]
fn staticcall_on_precompile_ecadd_with_not_enough_gas() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 128;
    let args_size: u8 = 128;
    let args_offset: u8 = 0;
    let gas: u32 = 149;
    let callee_address = Address::from_low_u64_be(ECADD_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 2;
    let x2: u8 = 1;
    let y2: u8 = 2;

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, x2.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y2.into())),
        Operation::Push((1_u8, 0x60_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_num_result(env, db, BigUint::ZERO);
}

#[test]
fn staticcall_on_precompile_ecmul_happy_path() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 96;
    let args_size: u8 = 96;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECMUL_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 2;
    let s: u8 = 2;

    let expected_x =
        hex::decode("030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd3").unwrap();
    let expected_y =
        hex::decode("15ed738c0e0a7c92e7845f96b2ae9c0a68a6a449e3538fc7ff3ebf7a5a18a2c4").unwrap();
    let expected_result = [expected_x, expected_y].concat();

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, s.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), //Ret size
        Operation::Push((1_u8, ret_offset.into())), //Ret offset
        Operation::Push((1_u8, args_size.into())), //Args size
        Operation::Push((1_u8, args_offset.into())), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, gas.into())),     //Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecmul_infinity() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 96;
    let args_size: u8 = 96;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECMUL_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 0;
    let y1: u8 = 0;
    let s: u8 = 2;

    let expected_result = Bytes::from([0u8; 64].to_vec());

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, s.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecmul_by_zero() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 96;
    let args_size: u8 = 96;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECMUL_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 2;
    let s: u8 = 0;

    let expected_result = Bytes::from([0u8; 64].to_vec());

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, s.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecmul_invalid_point() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 96;
    let args_size: u8 = 96;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECMUL_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 1;
    let s: u8 = 2;

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, s.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_num_result(env, db, BigUint::ZERO);
}

#[test]
fn staticcall_on_precompile_ecmul_invalid_point_by_zero() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 96;
    let args_size: u8 = 96;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECMUL_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 1;
    let s: u8 = 0;

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, s.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_num_result(env, db, BigUint::ZERO);
}

#[test]
fn staticcall_on_precompile_ecmul_with_missing_stack_parameter() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 96;
    let args_size: u8 = 96;
    let args_offset: u8 = 0;
    let gas: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECMUL_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 2;
    let s: u8 = 2;

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, s.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        // Do the call
        // Operation::Push((1_u8, ret_size.into())), // Ret size missing
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())),  // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),       // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_halt(env, db);
}

#[test]
fn staticcall_on_precompile_ecmul_with_not_enough_gas() {
    let ret_size: u8 = 64;
    let ret_offset: u8 = 96;
    let args_size: u8 = 96;
    let args_offset: u8 = 0;
    let gas: u32 = 5999;
    let callee_address = Address::from_low_u64_be(ECMUL_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let x1: u8 = 1;
    let y1: u8 = 2;
    let s: u8 = 2;

    let caller_ops = vec![
        // Store the parameters in memory
        Operation::Push((32_u8, x1.into())),
        Operation::Push((1_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, y1.into())),
        Operation::Push((1_u8, 0x20_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, s.into())),
        Operation::Push((1_u8, 0x40_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas.into())),     // Gas
        Operation::StaticCall,
        // Return
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_num_result(env, db, BigUint::ZERO);
}

#[test]
fn staticcall_on_precompile_ecpairing_happy_path() {
    let ret_size: u16 = 32;
    let ret_offset: u16 = 128;
    let args_size: u16 = 384;
    let args_offset: u16 = 0;
    let gas_limit: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECPAIRING_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let calldata = Bytes::from(
        hex::decode(
            "\
        2cf44499d5d27bb186308b7af7af02ac5bc9eeb6a3d147c186b21fb1b76e18da\
        2c0f001f52110ccfe69108924926e45f0b0c868df0e7bde1fe16d3242dc715f6\
        1fb19bb476f6b9e44e2a32234da8212f61cd63919354bc06aef31e3cfaff3ebc\
        22606845ff186793914e03e21df544c34ffe2f2f3504de8a79d9159eca2d98d9\
        2bd368e28381e8eccb5fa81fc26cf3f048eea9abfdd85d7ed3ab3698d63e4f90\
        2fe02e47887507adf0ff1743cbac6ba291e66f59be6bd763950bb16041a0a85e\
        0000000000000000000000000000000000000000000000000000000000000001\
        30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd45\
        1971ff0471b09fa93caaf13cbf443c1aede09cc4328f5a62aad45f40ec133eb4\
        091058a3141822985733cbdddfed0fd8d6c104e9e9eff40bf5abfef9ab163bc7\
        2a23af9a5ce2ba2796c1f4e453a370eb0af8c212d9dc9acd8fc02c2e907baea2\
        23a8eb0b0996252cb548a4487da97b02422ebc0e834613f954de6c7e0afdc1fc",
        )
        .unwrap(),
    );

    let expected_result =
        hex::decode("0000000000000000000000000000000000000000000000000000000000000001").unwrap();

    let mut caller_ops = vec![];

    // Store the entire calldata in memory (384 bytes, broken into 12 chunks of 32 bytes)
    for i in 0..12 {
        caller_ops.push(Operation::Push((
            32_u8,
            BigUint::from_bytes_be(&calldata[i * 32..(i + 1) * 32]),
        )));
        caller_ops.push(Operation::Push((2_u8, ((i * 32) as u16).into()))); // Adjusted for u16 offset
        caller_ops.push(Operation::Mstore);
    }

    caller_ops.extend(vec![
        // Do the call
        Operation::Push((2_u8, ret_size.into())), // Ret size
        Operation::Push((2_u8, ret_offset.into())), // Ret offset
        Operation::Push((2_u8, args_size.into())), // Args size
        Operation::Push((2_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas_limit.into())), // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((2_u8, ret_size.into())),
        Operation::Push((2_u8, ret_offset.into())),
        Operation::Return,
    ]);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecpairing_p1_is_infinity() {
    let ret_size: u8 = 32;
    let ret_offset: u8 = 128;
    let args_size: u8 = 192;
    let args_offset: u8 = 0;
    let gas_limit: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECPAIRING_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let calldata = Bytes::from(
        hex::decode(
            "\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            1fb19bb476f6b9e44e2a32234da8212f61cd63919354bc06aef31e3cfaff3ebc\
            22606845ff186793914e03e21df544c34ffe2f2f3504de8a79d9159eca2d98d9\
            2bd368e28381e8eccb5fa81fc26cf3f048eea9abfdd85d7ed3ab3698d63e4f90\
            2fe02e47887507adf0ff1743cbac6ba291e66f59be6bd763950bb16041a0a85e",
        )
        .unwrap(),
    );

    let expected_result =
        hex::decode("0000000000000000000000000000000000000000000000000000000000000001").unwrap();

    let mut caller_ops = vec![];

    // Store the entire calldata in memory (192 bytes, broken into 6 chunks of 32 bytes)
    for i in 0..6 {
        caller_ops.push(Operation::Push((
            32_u8,
            BigUint::from_bytes_be(&calldata[i * 32..(i + 1) * 32]),
        )));
        caller_ops.push(Operation::Push((1_u8, ((i * 32) as u8).into())));
        caller_ops.push(Operation::Mstore);
    }

    caller_ops.extend(vec![
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas_limit.into())), // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ]);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecpairing_p2_is_infinity() {
    let ret_size: u8 = 32;
    let ret_offset: u8 = 128;
    let args_size: u8 = 192;
    let args_offset: u8 = 0;
    let gas_limit: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECPAIRING_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let calldata = Bytes::from(
        hex::decode(
            "\
            2cf44499d5d27bb186308b7af7af02ac5bc9eeb6a3d147c186b21fb1b76e18da\
            2c0f001f52110ccfe69108924926e45f0b0c868df0e7bde1fe16d3242dc715f6\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap(),
    );

    let expected_result =
        hex::decode("0000000000000000000000000000000000000000000000000000000000000001").unwrap();

    let mut caller_ops = vec![];

    // Store the entire calldata in memory (192 bytes, broken into 6 chunks of 32 bytes)
    for i in 0..6 {
        caller_ops.push(Operation::Push((
            32_u8,
            BigUint::from_bytes_be(&calldata[i * 32..(i + 1) * 32]),
        )));
        caller_ops.push(Operation::Push((1_u8, ((i * 32) as u8).into())));
        caller_ops.push(Operation::Mstore);
    }

    caller_ops.extend(vec![
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas_limit.into())), // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ]);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecpairing_empty_calldata() {
    let ret_size: u8 = 32;
    let ret_offset: u8 = 128;
    let args_size: u8 = 0;
    let args_offset: u8 = 0;
    let gas_limit: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECPAIRING_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let expected_result =
        hex::decode("0000000000000000000000000000000000000000000000000000000000000001").unwrap();

    let mut caller_ops = vec![];

    caller_ops.extend(vec![
        // Do the call
        Operation::Push((1_u8, ret_size.into())), // Ret size
        Operation::Push((1_u8, ret_offset.into())), // Ret offset
        Operation::Push((1_u8, args_size.into())), // Args size
        Operation::Push((1_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas_limit.into())), // Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ]);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn staticcall_on_precompile_ecpairing_invalid_point() {
    let ret_size: u16 = 32;
    let ret_offset: u16 = 128;
    let args_size: u16 = 384;
    let args_offset: u16 = 0;
    let gas_limit: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECPAIRING_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    // changed last byte from `fc` to `fd`
    let calldata = Bytes::from(
        hex::decode(
            "\
        2cf44499d5d27bb186308b7af7af02ac5bc9eeb6a3d147c186b21fb1b76e18da\
        2c0f001f52110ccfe69108924926e45f0b0c868df0e7bde1fe16d3242dc715f6\
        1fb19bb476f6b9e44e2a32234da8212f61cd63919354bc06aef31e3cfaff3ebc\
        22606845ff186793914e03e21df544c34ffe2f2f3504de8a79d9159eca2d98d9\
        2bd368e28381e8eccb5fa81fc26cf3f048eea9abfdd85d7ed3ab3698d63e4f90\
        2fe02e47887507adf0ff1743cbac6ba291e66f59be6bd763950bb16041a0a85e\
        0000000000000000000000000000000000000000000000000000000000000001\
        30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd45\
        1971ff0471b09fa93caaf13cbf443c1aede09cc4328f5a62aad45f40ec133eb4\
        091058a3141822985733cbdddfed0fd8d6c104e9e9eff40bf5abfef9ab163bc7\
        2a23af9a5ce2ba2796c1f4e453a370eb0af8c212d9dc9acd8fc02c2e907baea2\
        23a8eb0b0996252cb548a4487da97b02422ebc0e834613f954de6c7e0afdc1fd",
        )
        .unwrap(),
    );

    let mut caller_ops = vec![];

    // Store the entire calldata in memory (384 bytes, broken into 12 chunks of 32 bytes)
    for i in 0..12 {
        caller_ops.push(Operation::Push((
            32_u8,
            BigUint::from_bytes_be(&calldata[i * 32..(i + 1) * 32]),
        )));
        caller_ops.push(Operation::Push((2_u8, ((i * 32) as u16).into()))); // Adjusted for u16 offset
        caller_ops.push(Operation::Mstore);
    }

    caller_ops.extend(vec![
        // Do the call
        Operation::Push((2_u8, ret_size.into())), // Ret size
        Operation::Push((2_u8, ret_offset.into())), // Ret offset
        Operation::Push((2_u8, args_size.into())), // Args size
        Operation::Push((2_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas_limit.into())), // Gas
        Operation::StaticCall,
        // Return
    ]);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_num_result(env, db, BigUint::ZERO);
}

#[test]
fn staticcall_on_precompile_ecpairing_invalid_calldata() {
    let ret_size: u16 = 32;
    let ret_offset: u16 = 128;
    let args_size: u16 = 384;
    let args_offset: u16 = 0;
    let gas_limit: u32 = 100_000_000;
    let callee_address = Address::from_low_u64_be(ECPAIRING_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    // deleted last value
    let calldata = Bytes::from(
        hex::decode(
            "\
        2cf44499d5d27bb186308b7af7af02ac5bc9eeb6a3d147c186b21fb1b76e18da\
        2c0f001f52110ccfe69108924926e45f0b0c868df0e7bde1fe16d3242dc715f6\
        1fb19bb476f6b9e44e2a32234da8212f61cd63919354bc06aef31e3cfaff3ebc\
        22606845ff186793914e03e21df544c34ffe2f2f3504de8a79d9159eca2d98d9\
        2bd368e28381e8eccb5fa81fc26cf3f048eea9abfdd85d7ed3ab3698d63e4f90\
        2fe02e47887507adf0ff1743cbac6ba291e66f59be6bd763950bb16041a0a85e\
        0000000000000000000000000000000000000000000000000000000000000001\
        30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd45\
        1971ff0471b09fa93caaf13cbf443c1aede09cc4328f5a62aad45f40ec133eb4\
        091058a3141822985733cbdddfed0fd8d6c104e9e9eff40bf5abfef9ab163bc7\
        2a23af9a5ce2ba2796c1f4e453a370eb0af8c212d9dc9acd8fc02c2e907baea2",
        )
        .unwrap(),
    );

    let mut caller_ops = vec![];

    // Store the incomplete calldata in memory (352 bytes, broken into 11 chunks of 32 bytes)
    for i in 0..11 {
        caller_ops.push(Operation::Push((
            32_u8,
            BigUint::from_bytes_be(&calldata[i * 32..(i + 1) * 32]),
        )));
        caller_ops.push(Operation::Push((2_u8, ((i * 32) as u16).into()))); // Adjusted for u16 offset
        caller_ops.push(Operation::Mstore);
    }

    caller_ops.extend(vec![
        // Do the call
        Operation::Push((2_u8, ret_size.into())), // Ret size
        Operation::Push((2_u8, ret_offset.into())), // Ret offset
        Operation::Push((2_u8, args_size.into())), // Args size
        Operation::Push((2_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas_limit.into())), // Gas
        Operation::StaticCall,
        // Return
    ]);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_num_result(env, db, BigUint::ZERO);
}

#[test]
fn staticcall_on_precompile_ecpairing_with_not_enough_gas() {
    let ret_size: u16 = 32;
    let ret_offset: u16 = 128;
    let args_size: u16 = 384;
    let args_offset: u16 = 0;
    // needs 113_000
    let gas_limit: u32 = 100_000;
    let callee_address = Address::from_low_u64_be(ECPAIRING_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let calldata = Bytes::from(
        hex::decode(
            "\
        2cf44499d5d27bb186308b7af7af02ac5bc9eeb6a3d147c186b21fb1b76e18da\
        2c0f001f52110ccfe69108924926e45f0b0c868df0e7bde1fe16d3242dc715f6\
        1fb19bb476f6b9e44e2a32234da8212f61cd63919354bc06aef31e3cfaff3ebc\
        22606845ff186793914e03e21df544c34ffe2f2f3504de8a79d9159eca2d98d9\
        2bd368e28381e8eccb5fa81fc26cf3f048eea9abfdd85d7ed3ab3698d63e4f90\
        2fe02e47887507adf0ff1743cbac6ba291e66f59be6bd763950bb16041a0a85e\
        0000000000000000000000000000000000000000000000000000000000000001\
        30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd45\
        1971ff0471b09fa93caaf13cbf443c1aede09cc4328f5a62aad45f40ec133eb4\
        091058a3141822985733cbdddfed0fd8d6c104e9e9eff40bf5abfef9ab163bc7\
        2a23af9a5ce2ba2796c1f4e453a370eb0af8c212d9dc9acd8fc02c2e907baea2\
        23a8eb0b0996252cb548a4487da97b02422ebc0e834613f954de6c7e0afdc1fc",
        )
        .unwrap(),
    );

    let mut caller_ops = vec![];

    // Store the entire calldata in memory (384 bytes, broken into 12 chunks of 32 bytes)
    for i in 0..12 {
        caller_ops.push(Operation::Push((
            32_u8,
            BigUint::from_bytes_be(&calldata[i * 32..(i + 1) * 32]),
        )));
        caller_ops.push(Operation::Push((2_u8, ((i * 32) as u16).into()))); // Adjusted for u16 offset
        caller_ops.push(Operation::Mstore);
    }

    caller_ops.extend(vec![
        // Do the call
        Operation::Push((2_u8, ret_size.into())), // Ret size
        Operation::Push((2_u8, ret_offset.into())), // Ret offset
        Operation::Push((2_u8, args_size.into())), // Args size
        Operation::Push((2_u8, args_offset.into())), // Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), // Address
        Operation::Push((32_u8, gas_limit.into())), // Gas
        Operation::StaticCall,
        // Return
    ]);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_num_result(env, db, BigUint::ZERO);
}

#[test]
fn staticcall_on_precompile_blake2f_happy_path() {
    let gas = 100_000_000_u32;
    let args_offset = 0_u8;
    let args_size = 213_u8;
    let ret_offset = 0_u8;
    let ret_size = 64_u8;

    // 4 bytes
    let rounds = hex::decode("0000000c").unwrap();
    // 64 bytes
    let h = hex::decode("48c9bdf267e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5d182e6ad7f520e511f6c3e2b8c68059b6bbd41fbabd9831f79217e1319cde05b").unwrap();
    // 128 bytes
    let m = hex::decode("6162630000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000").unwrap();
    // 16 bytes
    let t = hex::decode("03000000000000000000000000000000").unwrap();
    // 1 bytes
    let f = hex::decode("01").unwrap();

    // Reach 32 bytes multiple
    let padding = vec![0_u8; 11];

    let calldata = [rounds, h, m, t, f, padding].concat();

    let callee_address = Address::from_low_u64_be(BLAKE2F_ADDRESS);
    let caller_address = Address::from_low_u64_be(4040);

    let expected_result = hex::decode(
        "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
    ).unwrap();

    let caller_ops = vec![
        // Place the parameters in memory
        // rounds - 4 bytes
        Operation::Push((32_u8, BigUint::from_bytes_be(&calldata[..32]))),
        Operation::Push((32_u8, 0_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(&calldata[32..64]))),
        Operation::Push((32_u8, 32_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(&calldata[64..96]))),
        Operation::Push((32_u8, 64_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(&calldata[96..128]))),
        Operation::Push((32_u8, 96_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(&calldata[128..160]))),
        Operation::Push((32_u8, 128_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(&calldata[160..192]))),
        Operation::Push((32_u8, 160_u8.into())),
        Operation::Mstore,
        Operation::Push((32_u8, BigUint::from_bytes_be(&calldata[192..224]))),
        Operation::Push((32_u8, 192_u8.into())),
        Operation::Mstore,
        // Do the call
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, BigUint::from(gas))),     //Gas
        Operation::StaticCall,
        // Return
        Operation::Push((1_u8, ret_size.into())),
        Operation::Push((1_u8, ret_offset.into())),
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = Db::new().with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);

    run_program_assert_bytes_result(env, db, &expected_result);
}

#[test]
fn extcodehash_happy_path() {
    let address_number = 10;
    let mut operations = vec![
        Operation::Push((1, BigUint::from(address_number))),
        Operation::ExtcodeHash,
    ];
    append_return_result_operations(&mut operations);
    let (env, mut db) = default_env_and_db_setup(operations);
    let bytecode = Bytecode::from_static(b"60806040");
    let address = Address::from_low_u64_be(address_number);
    db = db.with_contract(address, bytecode);

    let code_hash = db.basic(address).unwrap().unwrap().code_hash;
    let expected_code_hash = BigUint::from_bytes_be(code_hash.as_bytes());

    run_program_assert_num_result(env, db, expected_code_hash);
}

#[test]
fn extcodehash_with_stack_underflow() {
    let operations = vec![Operation::ExtcodeHash];
    let (env, db) = default_env_and_db_setup(operations);

    run_program_assert_halt(env, db);
}

#[test]
fn extcodehash_with_32_byte_address() {
    // When the address is pushed as a 32 byte value, only the last 20 bytes should be used to load the address.
    let address_number = 10;
    let mut address_bytes = [0xff; 32];
    address_bytes[12..].copy_from_slice(Address::from_low_u64_be(address_number).as_bytes());
    let mut operations = vec![
        Operation::Push((32, BigUint::from_bytes_be(&address_bytes))),
        Operation::ExtcodeHash,
    ];
    append_return_result_operations(&mut operations);
    let (env, mut db) = default_env_and_db_setup(operations);
    let bytecode = Bytecode::from_static(b"60806040");
    let address = Address::from_low_u64_be(address_number);
    db = db.with_contract(address, bytecode);

    let code_hash = db.basic(address).unwrap().unwrap().code_hash;
    let expected_code_hash = BigUint::from_bytes_be(code_hash.as_bytes());

    run_program_assert_num_result(env, db, expected_code_hash);
}

#[test]
fn extcodehash_with_non_existent_address() {
    let address_number: u8 = 10;
    let mut operations = vec![
        Operation::Push((1, BigUint::from(address_number))),
        Operation::ExtcodeHash,
    ];
    append_return_result_operations(&mut operations);
    let (env, db) = default_env_and_db_setup(operations);
    let expected_code_hash = BigUint::ZERO;

    run_program_assert_num_result(env, db, expected_code_hash);
}

#[test]
fn extcodehash_address_with_no_code() {
    let address_number = 10;
    let empty_keccak =
        hex::decode("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470").unwrap();

    let mut operations = vec![
        Operation::Push((1, BigUint::from(address_number))),
        Operation::ExtcodeHash,
    ];
    append_return_result_operations(&mut operations);
    let (env, mut db) = default_env_and_db_setup(operations);

    let bytecode = Bytecode::from_static(b"");
    let address = Address::from_low_u64_be(address_number);
    db = db.with_contract(address, bytecode);
    let expected_code_hash = BigUint::from_bytes_be(&empty_keccak);

    run_program_assert_num_result(env, db, expected_code_hash);
}

#[test]
fn returndatasize_happy_path() {
    // Callee
    let mut callee_ops = vec![Operation::Push0];
    append_return_result_operations(&mut callee_ops);

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = Db::default().with_contract(callee_address, bytecode);

    let gas = 100_u8;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

    let caller_address = Address::from_low_u64_be(4040);
    let mut caller_ops = vec![
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((1_u8, BigUint::from(value))),    //Value
        Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((1_u8, BigUint::from(gas))),      //Gas
        Operation::Call,
        Operation::ReturnDataSize,
    ];

    append_return_result_operations(&mut caller_ops);

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let db = db.with_contract(caller_address, bytecode);

    let expected_result = 32_u8.into();

    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn returndatasize_no_return_data() {
    let caller_address = Address::from_low_u64_be(4040);
    let mut caller_ops = vec![Operation::ReturnDataSize];

    append_return_result_operations(&mut caller_ops);

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let db = Db::default().with_contract(caller_address, bytecode);

    let expected_result = 0_u8.into();

    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn returndatasize_gas_check() {
    let operations = vec![Operation::ReturnDataSize];
    let (env, db) = default_env_and_db_setup(operations);
    let needed_gas = gas_cost::RETURNDATASIZE as _;

    run_program_assert_gas_exact_with_db(env, db, needed_gas)
}

#[test]
fn returndatacopy_happy_path() {
    // Callee
    let return_value = 15_u8;
    let mut callee_ops = vec![Operation::Push((1_u8, return_value.into()))];
    append_return_result_operations(&mut callee_ops);

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = Db::default().with_contract(callee_address, bytecode);

    // Call arguments
    let gas = 100_u8;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

    // ReturnDataCopy arguments
    let dest_offset = 0_u8;
    let offset = 0_u8;
    let size = 32_u8;

    let caller_address = Address::from_low_u64_be(4040);
    let caller_ops = vec![
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((1_u8, BigUint::from(value))),    //Value
        Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((1_u8, BigUint::from(gas))),      //Gas
        Operation::Call,
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::ReturnDataCopy,
        // Return 32 bytes of data
        Operation::Push((1_u8, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let db = db.with_contract(caller_address, bytecode);

    let expected_result = return_value.into();

    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn returndatacopy_no_return_data() {
    let caller_address = Address::from_low_u64_be(4040);

    // ReturnDataCopy arguments
    let dest_offset = 0_u8;
    let offset = 0_u8;
    let size = 0_u8;

    let initial_memory_state = [0xFF_u8; 32]; //All 1s
    let caller_ops = vec![
        // Set up memory with all 1s
        Operation::Push((32_u8, BigUint::from_bytes_be(&initial_memory_state))),
        Operation::Push0,
        Operation::Mstore,
        // ReturnDataCopy operation
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::ReturnDataCopy,
        // Return 0 bytes of data
        Operation::Push((1_u8, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let db = Db::default().with_contract(caller_address, bytecode);

    // There was no return data, so memory stays the same
    let expected_result = &initial_memory_state;

    run_program_assert_bytes_result(env, db, expected_result);
}

#[test]
fn returndatacopy_size_smaller_than_data() {
    // Callee
    let return_data: &[u8] = &[0x33, 0x44, 0x55, 0x66, 0x77];

    // Callee
    let callee_ops = vec![
        Operation::Push((5_u8, BigUint::from_bytes_be(return_data))),
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1_u8, 5_u8.into())),
        Operation::Push((1_u8, 27_u8.into())),
        Operation::Return,
    ];

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = Db::default().with_contract(callee_address, bytecode);

    // Call arguments
    let gas = 100_u8;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

    // ReturnDataCopy arguments
    let dest_offset = 1_u8;
    let offset = 1_u8;
    let size = 3_u8;

    let initial_memory_state = [0xFF_u8; 32]; //All 1s
    let caller_address = Address::from_low_u64_be(4040);
    let caller_ops = vec![
        // Set up memory with all 1s
        Operation::Push((32_u8, BigUint::from_bytes_be(&initial_memory_state))),
        Operation::Push0,
        Operation::Mstore,
        // Make the call
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((1_u8, BigUint::from(value))),    //Value
        Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((1_u8, BigUint::from(gas))),      //Gas
        Operation::Call,
        // Use ReturnDataCopy
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::ReturnDataCopy,
        // Return 32 bytes of data
        Operation::Push((1_u8, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let db = db.with_contract(caller_address, bytecode);

    let expected_result = &[
        vec![0xFF_u8; 1], // <No collapse>
        vec![0x44, 0x55, 0x66],
        vec![0xFF_u8; 28],
    ]
    .concat();

    run_program_assert_bytes_result(env, db, expected_result);
}

#[test]
fn returndatacopy_with_offset_and_size_bigger_than_data() {
    // Callee
    let return_value = 15_u8;
    let mut callee_ops = vec![Operation::Push((1_u8, return_value.into()))];
    append_return_result_operations(&mut callee_ops);

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = Db::default().with_contract(callee_address, bytecode);

    // Call arguments
    let gas = 100_u8;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

    // ReturnDataCopy arguments
    let dest_offset = 0_u8;
    let offset = 10_u8;
    let size = 23_u8;

    let caller_address = Address::from_low_u64_be(4040);
    let caller_ops = vec![
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((1_u8, BigUint::from(value))),    //Value
        Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((1_u8, BigUint::from(gas))),      //Gas
        Operation::Call,
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::ReturnDataCopy,
    ];

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let db = db.with_contract(caller_address, bytecode);

    run_program_assert_halt(env, db);
}

#[test]
fn returndatacopy_check_stack_underflow() {
    let program = vec![Operation::ReturnDataCopy];
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn returndatacopy_gas_check() {
    // Callee
    let return_value = 15_u8;
    let callee_ops = vec![
        Operation::Push((1_u8, return_value.into())),
        // Return value
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ];

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = Db::default().with_contract(callee_address, bytecode);

    // Call arguments
    let gas = 100_u8;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

    // ReturnDataCopy arguments
    let dest_offset = 32_u8;
    let offset = 0_u8;
    let size = 32_u8;

    let caller_address = Address::from_low_u64_be(4040);
    let caller_ops = vec![
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((1_u8, BigUint::from(value))),    //Value
        Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((1_u8, BigUint::from(gas))),      //Gas
        Operation::Call,
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::ReturnDataCopy,
    ];

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let db = db.with_contract(caller_address, bytecode);

    let callee_gas_cost = gas_cost::PUSHN * 2
        + gas_cost::PUSH0 * 2
        + gas_cost::MSTORE
        + gas_cost::memory_expansion_cost(0, 32_u32); // Return data
    let caller_gas_cost = gas_cost::PUSHN * 10
        + gas_cost::CALL
        + call_opcode::WARM_MEMORY_ACCESS_COST as i64
        + gas_cost::memory_copy_cost(size.into())
        + gas_cost::memory_expansion_cost(0, (dest_offset + size) as u32)
        + gas_cost::RETURNDATACOPY;

    let initial_gas = 1e5;
    let consumed_gas = caller_gas_cost + callee_gas_cost;

    run_program_assert_gas_and_refund(env, db, initial_gas as _, consumed_gas as _, 0);
}

#[test]
fn create_happy_path() {
    let value: u8 = 10;
    let offset: u8 = 19;
    let size: u8 = 13;
    let sender_nonce = 1;
    let sender_balance = EU256::from(25);
    let sender_addr = Address::from_low_u64_be(40);

    // Code that returns the value 0xffffffff
    let initialization_code = hex::decode("63FFFFFFFF6000526004601CF3").unwrap();
    let bytecode = [0xff, 0xff, 0xff, 0xff];
    let mut hasher = Keccak256::new();
    hasher.update(bytecode);
    let initialization_code_hash = B256::from_slice(&hasher.finalize());

    let mut operations = vec![
        // Store initialization code in memory
        Operation::Push((13, BigUint::from_bytes_be(&initialization_code))),
        Operation::Push((1, BigUint::ZERO)),
        Operation::Mstore,
        // Create
        Operation::Push((1, BigUint::from(size))),
        Operation::Push((1, BigUint::from(offset))),
        Operation::Push((1, BigUint::from(value))),
        Operation::Create,
    ];
    append_return_result_operations(&mut operations);
    let (mut env, mut db) = default_env_and_db_setup(operations);
    db.set_account(
        sender_addr,
        sender_nonce,
        sender_balance,
        Default::default(),
    );
    env.tx.value = EU256::from(value);
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());

    // Check that contract is created correctly in the returned address
    let returned_addr = Address::from_slice(&result.output().unwrap()[12..]);
    let new_account = evm.db.basic(returned_addr).unwrap().unwrap();
    assert_eq!(new_account.balance, EU256::from(value));
    assert_eq!(new_account.nonce, 1);
    assert_eq!(new_account.code_hash, initialization_code_hash);
    let new_account_code = evm.db.code_by_hash(new_account.code_hash).unwrap();
    assert_ne!(new_account_code, Bytecode::default());

    // Check that the sender account is updated
    let sender_account = evm.db.basic(sender_addr).unwrap().unwrap();
    assert_eq!(sender_account.nonce, sender_nonce + 1);
    assert_eq!(sender_account.balance, sender_balance - value);
}

#[test]
fn create_with_stack_underflow() {
    let operations = vec![Operation::Create];
    let (env, db) = default_env_and_db_setup(operations);

    run_program_assert_halt(env, db);
}

#[test]
fn create_with_balance_underflow() {
    let value: u8 = 10;
    let offset: u8 = 19;
    let size: u8 = 13;
    let sender_nonce = 1;
    let sender_balance = EU256::zero();
    let sender_addr = Address::from_low_u64_be(40);

    // Code that returns the value 0xffffffff
    let initialization_code = hex::decode("63FFFFFFFF6000526004601CF3").unwrap();

    let mut operations = vec![
        // Store initialization code in memory
        Operation::Push((13, BigUint::from_bytes_be(&initialization_code))),
        Operation::Push((1, BigUint::ZERO)),
        Operation::Mstore,
        // Create
        Operation::Push((1, BigUint::from(size))),
        Operation::Push((1, BigUint::from(offset))),
        Operation::Push((1, BigUint::from(value))),
        Operation::Create,
    ];
    append_return_result_operations(&mut operations);
    let (mut env, mut db) = default_env_and_db_setup(operations);
    db.set_account(
        sender_addr,
        sender_nonce,
        sender_balance,
        Default::default(),
    );
    env.tx.value = EU256::from(value);
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();

    // Check that the result is zero
    assert!(result.is_success());
    assert_eq!(result.output().unwrap().to_vec(), [0_u8; 32].to_vec());

    // Check that the sender account is not updated
    let sender_account = evm.db.basic(sender_addr).unwrap().unwrap();
    assert_eq!(sender_account.nonce, sender_nonce);
    assert_eq!(sender_account.balance, sender_balance);
}

#[test]
fn create_with_invalid_initialization_code() {
    let value: u8 = 0;
    let offset: u8 = 19;
    let size: u8 = 13;

    // Code that halts
    let initialization_code = hex::decode("63ffffffff526004601cf3").unwrap();
    let initialization_code_hash = B256::from_str(EMPTY_CODE_HASH_STR).unwrap();

    let mut operations = vec![
        // Store initialization code in memory
        Operation::Push((13, BigUint::from_bytes_be(&initialization_code))),
        Operation::Push((1, BigUint::ZERO)),
        Operation::Mstore,
        // Create
        Operation::Push((1, BigUint::from(size))),
        Operation::Push((1, BigUint::from(offset))),
        Operation::Push((1, BigUint::from(value))),
        Operation::Create,
    ];
    append_return_result_operations(&mut operations);
    let (mut env, db) = default_env_and_db_setup(operations);
    env.tx.value = EU256::from(value);
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();

    // Check that contract is created in the returned address with empty bytecode
    let returned_addr = Address::from_slice(&result.output().unwrap()[12..]);
    let new_account = evm.db.basic(returned_addr).unwrap().unwrap();
    assert_eq!(new_account.balance, EU256::from(value));
    assert_eq!(new_account.nonce, 1);
    assert_eq!(new_account.code_hash, initialization_code_hash);
}

#[test]
fn create_gas_cost() {
    let value: u8 = 0;
    let offset: u8 = 19;
    let size: u8 = 13;

    // Code that returns the value 0xffffffff
    let initialization_code = hex::decode("63FFFFFFFF6000526004601CF3").unwrap();
    let initialization_gas_cost: i64 = 18;
    let minimum_word_size: i64 = 1;
    let deployed_code_size: i64 = 4;

    let needed_gas = gas_cost::PUSHN * 4
        + gas_cost::PUSH0
        + gas_cost::MSTORE
        + gas_cost::memory_expansion_cost(0, (size + offset).into())
        + gas_cost::CREATE
        + initialization_gas_cost
        + gas_cost::INIT_WORD_COST * minimum_word_size
        + gas_cost::BYTE_DEPOSIT_COST * deployed_code_size;

    let operations = vec![
        // Store initialization code in memory
        Operation::Push((13, BigUint::from_bytes_be(&initialization_code))),
        Operation::Push0,
        Operation::Mstore,
        // Create
        Operation::Push((1, BigUint::from(size))),
        Operation::Push((1, BigUint::from(offset))),
        Operation::Push((1, BigUint::from(value))),
        Operation::Create,
    ];
    let (mut env, db) = default_env_and_db_setup(operations);
    env.tx.value = EU256::from(value);

    run_program_assert_gas_exact_with_db(env, db, needed_gas as _);
}

#[test]
fn create2_happy_path() {
    let value: u8 = 10;
    let offset: u8 = 19;
    let size: u8 = 13;
    let salt: u8 = 52;
    let sender_nonce = 1;
    let sender_balance = EU256::from(25);
    let sender_addr = Address::from_low_u64_be(40);

    // Code that returns the value 0xffffffff
    let initialization_code = hex::decode("63FFFFFFFF6000526004601CF3").unwrap();
    let bytecode = [0xff, 0xff, 0xff, 0xff];
    let mut hasher = Keccak256::new();
    hasher.update(bytecode);
    let initialization_code_hash = B256::from_slice(&hasher.finalize());

    let expected_address =
        compute_contract_address2(sender_addr, EU256::from(salt), &initialization_code);

    let mut operations = vec![
        // Store initialization code in memory
        Operation::Push((13, BigUint::from_bytes_be(&initialization_code))),
        Operation::Push((1, BigUint::ZERO)),
        Operation::Mstore,
        // Create
        Operation::Push((1, BigUint::from(salt))),
        Operation::Push((1, BigUint::from(size))),
        Operation::Push((1, BigUint::from(offset))),
        Operation::Push((1, BigUint::from(value))),
        Operation::Create2,
    ];
    append_return_result_operations(&mut operations);
    let (mut env, mut db) = default_env_and_db_setup(operations);
    db.set_account(
        sender_addr,
        sender_nonce,
        sender_balance,
        Default::default(),
    );
    env.tx.value = EU256::from(value);
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());

    // Check that the returned address is the expected
    let returned_addr = Address::from_slice(&result.output().unwrap()[12..]);
    assert_eq!(returned_addr, expected_address);

    // Check that contract is created correctly in the returned address
    let new_account = evm.db.basic(returned_addr).unwrap().unwrap();
    assert_eq!(new_account.balance, EU256::from(value));
    assert_eq!(new_account.nonce, 1);
    assert_eq!(new_account.code_hash, initialization_code_hash);

    // Check that the sender account is updated
    let sender_account = evm.db.basic(sender_addr).unwrap().unwrap();
    assert_eq!(sender_account.nonce, sender_nonce + 1);
    assert_eq!(sender_account.balance, sender_balance - value);
}

#[test]
fn create2_with_stack_underflow() {
    let operations = vec![Operation::Create2];
    let (env, db) = default_env_and_db_setup(operations);

    run_program_assert_halt(env, db);
}

fn staticcall_state_modifying_revert_with_callee_ops(callee_ops: Vec<Operation>) {
    let caller_address = Address::from_low_u64_be(4040);
    let db = Db::new();
    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let callee_balance = 100_u8;
    let mut db = db.with_contract(callee_address, bytecode);
    db.set_account(callee_address, 0, callee_balance.into(), Default::default());

    let gas = 1_000_000_u32;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

    let mut caller_ops = vec![
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((10_u8, BigUint::from(gas))),     //Gas
        Operation::StaticCall,
    ];

    append_return_result_operations(&mut caller_ops);

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let caller_balance = 100_u8;
    let mut db = db.with_contract(caller_address, bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());

    let expected_result = 0_u8.into();

    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn staticcall_with_selfdestruct_reverts() {
    let operations = vec![
        Operation::Push((1_u8, 1_u8.into())),
        Operation::SelfDestruct,
    ];
    staticcall_state_modifying_revert_with_callee_ops(operations);
}

#[test]
fn staticcall_with_sstore_reverts() {
    let operations = vec![
        Operation::Push((1_u8, 1_u8.into())),
        Operation::Push((1_u8, 1_u8.into())),
        Operation::Sstore,
    ];
    staticcall_state_modifying_revert_with_callee_ops(operations);
}

#[test]
fn staticcall_with_call_with_value_not_zero_reverts() {
    let operations = vec![
        Operation::Push((1_u8, 0_u8.into())), //Ret size
        Operation::Push((1_u8, 0_u8.into())), //Ret offset
        Operation::Push((1_u8, 0_u8.into())), //Args size
        Operation::Push((1_u8, 0_u8.into())), //Args offset
        Operation::Push((1_u8, 1_u8.into())), //Value
        Operation::Push((
            16_u8,
            BigUint::from_bytes_be(Address::from_low_u64_be(4040).as_bytes()),
        )),
        Operation::Push((32_u8, 1000_u32.into())), //Gas
        Operation::Call,
    ];
    staticcall_state_modifying_revert_with_callee_ops(operations);
}

#[rstest]
#[case(0)]
#[case(1)]
#[case(2)]
#[case(3)]
#[case(4)]
fn staticcall_with_call_with_log_reverts(#[case] nth: usize) {
    let mut operations = vec![Operation::Push((1_u8, 1_u8.into())); nth + 2];
    operations.push(Operation::Log(nth as u8));
    staticcall_state_modifying_revert_with_callee_ops(operations);
}

#[test]
fn staticcall_with_create_reverts() {
    let value: u8 = 10;
    let offset: u8 = 19;
    let size: u8 = 13;
    let initialization_code = hex::decode("63FFFFFFFF6000526004601CF3").unwrap();
    let mut operations = vec![
        // Store initialization code in memory
        Operation::Push((13, BigUint::from_bytes_be(&initialization_code))),
        Operation::Push((1, BigUint::ZERO)),
        Operation::Mstore,
        // Create
        Operation::Push((1, BigUint::from(value))),
        Operation::Push((1, BigUint::from(offset))),
        Operation::Push((1, BigUint::from(size))),
        Operation::Create,
    ];
    append_return_result_operations(&mut operations);
    staticcall_state_modifying_revert_with_callee_ops(operations)
}

#[test]
fn staticcall_with_create2_reverts() {
    let value: u8 = 10;
    let offset: u8 = 19;
    let size: u8 = 13;
    let salt: u8 = 52;
    // Code that returns the value 0xffffffff
    let initialization_code = hex::decode("63FFFFFFFF6000526004601CF3").unwrap();
    let mut operations = vec![
        // Store initialization code in memory
        Operation::Push((13, BigUint::from_bytes_be(&initialization_code))),
        Operation::Push((1, BigUint::ZERO)),
        Operation::Mstore,
        // Create
        Operation::Push((1, BigUint::from(salt))),
        Operation::Push((1, BigUint::from(size))),
        Operation::Push((1, BigUint::from(offset))),
        Operation::Push((1, BigUint::from(value))),
        Operation::Create2,
    ];
    append_return_result_operations(&mut operations);
    staticcall_state_modifying_revert_with_callee_ops(operations)
}

#[test]
fn staticcall_callee_returns_value() {
    let db = Db::new();
    let origin = Address::from_low_u64_be(79);
    let origin_value = 1_u8;

    // Callee
    let mut callee_ops = vec![Operation::Callvalue];
    append_return_result_operations(&mut callee_ops);

    // Caller
    let gas = 100_000_000_u32;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

    let program = Program::from(callee_ops);
    let (callee_address, callee_bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = db.with_contract(callee_address, callee_bytecode);

    let caller_address = Address::from_low_u64_be(4040);
    let mut caller_ops = vec![
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, BigUint::from(gas))),     //Gas
        Operation::StaticCall,
        //Return
        Operation::Push((1_u8, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ];

    append_return_result_operations(&mut caller_ops);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = db.with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = origin;
    env.tx.value = origin_value.into();

    let expected_result = 0_u8.into(); // Value is set to zero on a static call

    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn staticcall_callee_returns_caller() {
    let db = Db::new();
    let origin = Address::from_low_u64_be(79);

    // Callee
    let mut callee_ops = vec![Operation::Caller];
    append_return_result_operations(&mut callee_ops);

    // Caller
    let gas = 100_000_000_u32;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

    let program = Program::from(callee_ops);
    let (callee_address, callee_bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = db.with_contract(callee_address, callee_bytecode);

    let caller_address = Address::from_low_u64_be(4040);
    let mut caller_ops = vec![
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((20_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((32_u8, BigUint::from(gas))),     //Gas
        Operation::StaticCall,
        //Return
        Operation::Push((1_u8, 20_u8.into())),
        Operation::Push((1_u8, 12_u8.into())),
        Operation::Return,
    ];

    append_return_result_operations(&mut caller_ops);

    let program = Program::from(caller_ops);
    let caller_bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    let db = db.with_contract(caller_address, caller_bytecode);
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = origin;

    let expected_result = caller_address.as_fixed_bytes();

    run_program_assert_bytes_result(env, db, expected_result);
}

#[test]
fn selfdestruct_with_stack_underflow() {
    let operations = vec![Operation::SelfDestruct];
    let (env, db) = default_env_and_db_setup(operations);

    run_program_assert_halt(env, db);
}

#[test]
fn selfdestruct_happy_path() {
    // it should add the balance to the existing account
    let receiver_address = 100;
    let callee_balance = EU256::from(231);
    let receiver_balance = EU256::from(123);

    let operations = vec![
        Operation::Push((20, BigUint::from(receiver_address))),
        Operation::SelfDestruct,
    ];
    let (env, mut db) = default_env_and_db_setup(operations);
    let callee_address = env.tx.get_address();
    let receiver_address = Address::from_low_u64_be(receiver_address);
    db.set_account(callee_address, 1, callee_balance, Default::default());
    db.set_account(receiver_address, 1, receiver_balance, Default::default());
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());

    let callee = evm.db.basic(callee_address).unwrap().unwrap();
    let receiver = evm.db.basic(receiver_address).unwrap().unwrap();
    assert_eq!(callee.balance, EU256::zero());
    assert_eq!(receiver.balance, callee_balance + receiver_balance);
}

#[test]
fn selfdestruct_on_inexistent_address() {
    // it should add the balance to the address even if there is no existing account
    let receiver_address = 100;
    let balance = EU256::from(231);

    let operations = vec![
        Operation::Push((20, BigUint::from(receiver_address))),
        Operation::SelfDestruct,
    ];
    let (env, mut db) = default_env_and_db_setup(operations);
    let callee_address = env.tx.get_address();
    let receiver_address = Address::from_low_u64_be(receiver_address);
    db.set_account(callee_address, 1, balance, Default::default());
    let mut evm = Evm::new(env, db);

    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());

    let callee = evm.db.basic(callee_address).unwrap().unwrap();
    let receiver = evm.db.basic(receiver_address).unwrap().unwrap();
    assert_eq!(callee.balance, EU256::zero());
    assert_eq!(receiver.balance, balance);
}

#[test]
fn selfdestruct_on_already_existing_account() {
    // it should not be destructed, but modified (empty balance)
    let receiver_address = Address::from_low_u64_be(123);
    let caller_init_balance = 50_u8;

    let operations = vec![
        Operation::Push((20, BigUint::from_bytes_be(receiver_address.as_bytes()))),
        Operation::SelfDestruct,
    ];
    let (env, mut db) = default_env_and_db_setup(operations);

    let contract_address = env.tx.get_address();
    db.set_account(
        contract_address,
        1,
        caller_init_balance.into(),
        Default::default(),
    );

    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());

    let expected_contract_balance = 0.into();
    let expected_receiver_balance = caller_init_balance.into();

    assert!(evm.db.basic(contract_address).unwrap().is_some()); // It wasn't destroyed
    assert!(evm.db.basic(receiver_address).unwrap().is_some()); // It was created

    let contract_balance = evm.db.basic(contract_address).unwrap().unwrap().balance;
    let receiver_balance = evm.db.basic(receiver_address).unwrap().unwrap().balance;

    assert_eq!(contract_balance, expected_contract_balance);
    assert_eq!(receiver_balance, expected_receiver_balance);
}

#[test]
fn selfdestruct_on_newly_created_account() {
    let origin = Address::from_low_u64_be(123);
    let caller_address = Address::from_low_u64_be(4040);
    let caller_init_balance = 50_u8;
    let receiver_acc_address = Address::from_low_u64_be(3030);

    // Call args
    let gas = 100_000_u32;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

    // Create args
    let salt: u8 = 52;
    let init_code_size = 13_u8;
    let init_code_offset = 32_u8 - init_code_size;
    let transfer_value = 30_u8;

    // This bytecode will selfdestruct and transfer to `receiver_acc_address` (EVM Codes playground)
    let initialization_code = hex::decode("63610bd6ff6000526004601cf3").unwrap();
    let new_contract_address =
        compute_contract_address2(caller_address, EU256::from(salt), &initialization_code);
    let mut caller_ops = vec![
        // Store initialization code in memory
        Operation::Push((13, BigUint::from_bytes_be(&initialization_code))),
        Operation::Push((1, BigUint::ZERO)),
        Operation::Mstore,
        // Create
        Operation::Push((1, BigUint::from(salt))), // Salt
        Operation::Push((1, BigUint::from(init_code_size))), // Size
        Operation::Push((1, BigUint::from(init_code_offset))), // Offset
        Operation::Push((1, BigUint::from(transfer_value))), // Value to send
        Operation::Create2,
        // Make the call
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((1_u8, BigUint::from(value))),
        Operation::Push((
            20_u8,
            BigUint::from_bytes_be(new_contract_address.as_bytes()),
        )),
        Operation::Push((32_u8, BigUint::from(gas))), //Gas
        Operation::Call,
    ];
    append_return_result_operations(&mut caller_ops);

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = origin;
    env.tx.value = transfer_value.into();
    let mut db = Db::new().with_contract(caller_address, bytecode);
    db.set_account(
        caller_address,
        1,
        caller_init_balance.into(),
        Default::default(),
    );
    let mut evm = Evm::new(env, db);
    let result = evm.transact_commit().unwrap();
    assert!(result.is_success());
    let call_return_code = BigUint::from_bytes_be(result.output().unwrap());
    let expected_return_code = 1_u8.into();
    assert_eq!(call_return_code, expected_return_code);

    let expected_caller_balance = (caller_init_balance - transfer_value).into();
    let expected_receiver_balance = transfer_value.into();

    assert!(evm.db.basic(caller_address).unwrap().is_some());
    assert!(evm.db.basic(receiver_acc_address).unwrap().is_some());
    assert!(evm.db.basic(new_contract_address).unwrap().is_none());

    let caller_balance = evm.db.basic(caller_address).unwrap().unwrap().balance;
    let receiver_balance = evm.db.basic(receiver_acc_address).unwrap().unwrap().balance;

    assert_eq!(caller_balance, expected_caller_balance);
    assert_eq!(receiver_balance, expected_receiver_balance);
}

#[test]
fn selfdestruct_gas_cost_on_empty_account() {
    let receiver_address: u8 = 100;
    let needed_gas = gas_cost::PUSHN + gas_cost::SELFDESTRUCT;

    let operations = vec![
        Operation::Push((20, BigUint::from(receiver_address))),
        Operation::SelfDestruct,
    ];
    let env = Env::default();
    run_program_assert_gas_exact(operations, env, needed_gas as _);
}

#[test]
fn selfdestruct_gas_cost_on_non_empty_account() {
    let receiver_address: u8 = 100;
    let balance = EU256::from(231);
    let needed_gas = gas_cost::PUSHN + gas_cost::SELFDESTRUCT + gas_cost::SELFDESTRUCT_DYNAMIC_GAS;

    let operations = vec![
        Operation::Push((20, BigUint::from(receiver_address))),
        Operation::SelfDestruct,
    ];
    let (env, mut db) = default_env_and_db_setup(operations);
    let callee_address = env.tx.get_address();
    db.set_account(callee_address, 1, balance, Default::default());

    run_program_assert_gas_and_refund(env, db, needed_gas as _, needed_gas as _, 0);
}

#[test]
fn tload_gas_consumption() {
    let program = vec![
        Operation::Push((1_u8, BigUint::from(1_u8))),
        Operation::Tload,
    ];
    let needed_gas = gas_cost::PUSHN + gas_cost::TLOAD;
    let env = Env::default();

    run_program_assert_gas_exact(program, env, needed_gas as _);
}

#[test]
fn tload_on_new_key() {
    let program = vec![
        Operation::Push((1_u8, BigUint::from(5_u8))),
        Operation::Tload,
    ];
    let (env, db) = default_env_and_db_setup(program);
    let expected_result = BigUint::from(0_u8);
    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn tload_with_stack_underflow() {
    let program = vec![Operation::Tload];
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn tstore_gas_consumption() {
    let program = vec![
        Operation::Push((1_u8, BigUint::from(1_u8))),
        Operation::Push((1_u8, BigUint::from(2_u8))),
        Operation::Tstore,
    ];
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::TSTORE;
    let env = Env::default();

    run_program_assert_gas_exact(program, env, needed_gas as _);
}

#[test]
fn tstore_with_stack_underflow() {
    let program = vec![Operation::Push0, Operation::Tstore];
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn tstore_tload_happy_path() {
    let key = 80_u8;
    let value = 11_u8;
    let mut operations = vec![
        // tstore
        Operation::Push((1_u8, BigUint::from(value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Tstore,
        // tload
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Tload,
    ];
    append_return_result_operations(&mut operations);
    let (env, db) = default_env_and_db_setup(operations);
    run_program_assert_num_result(env, db, BigUint::from(value));
}

#[test]
fn sload_warm_cold_gas() {
    let used_gas = gas_cost::PUSHN * 2 + gas_cost::SLOAD_COLD + gas_cost::SLOAD_WARM;

    let program = vec![
        // first sload: gas_cost = cost_cold + cost_push
        Operation::Push((1_u8, BigUint::from(1_u8))),
        Operation::Sload,
        // second sload: gas_cost = cost_warm + cost_push
        Operation::Push((1_u8, BigUint::from(1_u8))),
        Operation::Sload,
    ];

    let env = Env::default();
    run_program_assert_gas_exact(program, env, used_gas as _);
}

#[test]
fn refunded_gas_cant_be_more_than_a_fifth_of_used_gas() {
    let new_value: u8 = 0;
    let original_value = 10;

    let used_gas = 5_000 + 2 * gas_cost::PUSHN;
    let needed_gas = used_gas + gas_cost::SSTORE_MIN_REMAINING_GAS;
    let refunded_gas = used_gas / GAS_REFUND_DENOMINATOR as i64;
    let key = 80_u8;
    let program = vec![
        Operation::Push((1_u8, BigUint::from(new_value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
    ];

    let (env, mut db) = default_env_and_db_setup(program);
    let callee = env.tx.get_address();
    db.write_storage(callee, EU256::from(key), EU256::from(original_value));

    run_program_assert_gas_and_refund(env, db, needed_gas as _, used_gas as _, refunded_gas as _);
}

#[test]
fn refund_limit_value() {
    let new_value: u8 = 0;
    let original_value = 10;

    let used_gas = 5_000 + (2 + 200) * gas_cost::PUSHN + gas_cost::BALANCE * 200;
    let needed_gas = used_gas + gas_cost::SSTORE_MIN_REMAINING_GAS;
    let refunded_gas = 4_800;
    let key = 80_u8;
    let gas_costly_opcodes = vec![Operation::Balance; 200];
    let needed_pushes_opcodes = vec![Operation::Push((20_u8, BigUint::from(1_u8))); 200];
    let program = [
        needed_pushes_opcodes,
        gas_costly_opcodes,
        vec![
            Operation::Push((1_u8, BigUint::from(new_value))),
            Operation::Push((1_u8, BigUint::from(key))),
            Operation::Sstore,
        ],
    ]
    .concat();

    let (env, mut db) = default_env_and_db_setup(program);
    let callee = env.tx.get_address();
    db.write_storage(callee, EU256::from(key), EU256::from(original_value));

    run_program_assert_gas_and_refund(env, db, needed_gas as _, used_gas as _, refunded_gas as _);
}
