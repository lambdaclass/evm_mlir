use rstest::rstest;
use std::{collections::HashMap, str::FromStr};

use evm_mlir::{
    constants::{call_opcode, gas_cost},
    db::{Bytecode, Database, Db},
    env::TransactTo,
    primitives::{Address, Bytes, B256, U256 as EU256},
    program::{Operation, Program},
    syscall::{LogData, U256},
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
    let db = Db::new().with_bytecode(address, bytecode);
    (env, db)
}

fn run_program_assert_num_result(env: Env, db: Db, expected_result: BigUint) {
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap().result;
    assert!(result.is_success());
    let result_data = BigUint::from_bytes_be(result.output().unwrap_or(&Bytes::new()));
    assert_eq!(result_data, expected_result);
}

fn run_program_assert_bytes_result(env: Env, db: Db, expected_result: &[u8]) {
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap().result;
    assert!(result.is_success());
    assert_eq!(result.output().unwrap().as_ref(), expected_result);
}

fn run_program_assert_halt(env: Env, db: Db) {
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap().result;
    assert!(result.is_halt());
}

fn run_program_assert_gas_exact_with_db(mut env: Env, db: Db, needed_gas: u64) {
    // Ok run
    env.tx.gas_limit = needed_gas;
    let mut evm = Evm::new(env.clone(), db.clone());
    let result = evm.transact().unwrap().result;
    assert!(result.is_success());

    // Halt run
    env.tx.gas_limit = needed_gas - 1;
    let mut evm = Evm::new(env.clone(), db);
    let result = evm.transact().unwrap().result;
    assert!(result.is_halt());
}

fn run_program_assert_gas_exact(operations: Vec<Operation>, env: Env, needed_gas: u64) {
    let address = env.tx.get_address();

    //Ok run
    let program = Program::from(operations.clone());
    let mut env_success = env.clone();
    env_success.tx.gas_limit = needed_gas;
    let db = Db::new().with_bytecode(address, program.to_bytecode().into());
    let mut evm = Evm::new(env_success, db);

    let result = evm.transact().unwrap().result;
    assert!(result.is_success());

    //Halt run
    let program = Program::from(operations.clone());
    let mut env_halt = env.clone();
    env_halt.tx.gas_limit = needed_gas - 1;
    let db = Db::new().with_bytecode(address, program.to_bytecode().into());
    let mut evm = Evm::new(env_halt, db);

    let result = evm.transact().unwrap().result;
    assert!(result.is_halt());
}

fn run_program_assert_gas_and_refund(
    mut env: Env,
    db: Db,
    needed_gas: u64,
    used_gas: u64,
    refunded_gas: u64,
) {
    env.tx.gas_limit = needed_gas;
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;
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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(Address::zero(), bytecode);
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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    env.tx.gas_limit = 1000;
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap().result;

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
    env.tx.gas_limit = 1000;
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap().result;

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
    env.tx.gas_limit = 1000;
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap().result;

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
    env.tx.gas_limit = 1000;
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    env.tx.gas_limit = 1000;
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let db = Db::new().with_bytecode(Address::zero(), bytecode);
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

    let (mut env, db) = default_env_and_db_setup(operations);
    let caller_address = Address::from_low_u64_be(41);
    env.tx.caller = caller_address;
    let mut evm = Evm::new(env, db);

    let res = evm.transact().unwrap();
    assert!(&res.result.is_success());

    let stored_value = res
        .state
        .get(&caller_address)
        .and_then(|account| account.storage.get(&EU256::from(key)))
        .map(|slot| slot.present_value)
        .unwrap_or(EU256::zero());

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
    let result = gas_cost::PUSHN + gas_cost::SLOAD;
    let env = Env::default();

    run_program_assert_gas_exact(program, env, result as _);
}

#[test]
fn sload_with_valid_key() {
    let key = 80_u8;
    let value = 11_u8;
    let program = Program::from(vec![
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sload,
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1_u8, BigUint::from(32_u8))),
        Operation::Push0,
        Operation::Return,
    ]);
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    let caller_address = Address::from_low_u64_be(41);
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.transact_to = TransactTo::Call(address);
    env.tx.caller = caller_address;
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    evm.db
        .write_storage(caller_address, EU256::from(key), EU256::from(value));

    let result = evm.transact().unwrap().result;
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

    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap().result;
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
    let mut db = Db::new().with_bytecode(address, bytecode);

    db.set_account(address, 0, balance, HashMap::new());

    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let mut db = Db::new().with_bytecode(address, bytecode);

    db.set_account(address, 0, balance, HashMap::new());

    let mut evm = Evm::new(env, db);

    let result = evm.transact().unwrap().result;

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
    let mut db = Db::new().with_bytecode(contract_address, bytecode);
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
    let mut db = Db::new().with_bytecode(contract_address, bytecode);
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
    env.tx.gas_limit = gaslimit;
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
    let refunded_gas = 4_800;

    let key = 80_u8;
    let program = vec![
        Operation::Push((1_u8, BigUint::from(new_value))),
        Operation::Push((1_u8, BigUint::from(key))),
        Operation::Sstore,
    ];

    let (env, mut db) = default_env_and_db_setup(program);
    db.write_storage(env.tx.caller, EU256::from(key), EU256::from(original_value));

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
    let refunded_gas = 2_800;

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
    db.write_storage(env.tx.caller, EU256::from(key), EU256::from(original_value));

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
    db.write_storage(env.tx.caller, EU256::from(key), EU256::from(original_value));

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
    let db = Db::new().with_bytecode(address, bytecode);
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
    let db = Db::new().with_bytecode(address, bytecode);
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
    let db = Db::new().with_bytecode(address, bytecode);
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
    let db = Db::new().with_bytecode(address, bytecode);
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
    let db = Db::new().with_bytecode(address, bytecode);
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

#[test]
fn call_returns_addition_from_arguments() {
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
    let db = db.with_bytecode(callee_address, bytecode);

    let gas = 100_u8;
    let value = 1_u8;
    let args_offset = 0_u8;
    let args_size = 64_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

    let caller_address = Address::from_low_u64_be(4040);
    let caller_ops = vec![
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
        Operation::Call,
        //This ops will return the value stored in memory, the call status and the caller balance
        //call status
        Operation::Push((1_u8, 32_u8.into())),
        Operation::Mstore,
        //caller balance
        Operation::Push((20_u8, BigUint::from_bytes_be(caller_address.as_bytes()))),
        Operation::Balance,
        Operation::Push((1_u8, 64_u8.into())),
        Operation::Mstore,
        //Return
        Operation::Push((1_u8, 96_u8.into())),
        Operation::Push0,
        Operation::Return,
    ];

    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let caller_balance = 100_u8;
    let mut db = db.with_bytecode(caller_address, bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());

    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap().result;
    assert!(result.is_success());

    let res_bytes: &[u8] = result.output().unwrap();

    let expected_contract_data_result = a + b;
    let expected_caller_balance_result = (caller_balance - value).into(); //TODO: Change
    let expected_contract_status_result = 1_u8.into(); //Call failed

    let contract_data_result = BigUint::from_bytes_be(&res_bytes[..32]);
    let contract_status_result = BigUint::from_bytes_be(&res_bytes[32..64]);
    let caller_balance_result = BigUint::from_bytes_be(&res_bytes[64..]);

    assert_eq!(contract_status_result, expected_contract_status_result);
    assert_eq!(contract_data_result, expected_contract_data_result);
    assert_eq!(caller_balance_result, expected_caller_balance_result);
}

#[test]
fn call_without_enough_balance() {
    let db = Db::new();

    // Callee
    let mut callee_ops = vec![Operation::Push0];
    append_return_result_operations(&mut callee_ops);

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = db.with_bytecode(callee_address, bytecode);

    let gas = 100_u8;
    let value = 1_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

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
        // Return both syscall result and caller balance
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((20_u8, BigUint::from_bytes_be(caller_address.as_bytes()))),
        Operation::Balance,
        Operation::Push((1_u8, 32_u8.into())),
        Operation::Mstore,
        Operation::Push((1_u8, 64_u8.into())),
        Operation::Push0,
        Operation::Return,
    ];

    let caller_balance: u8 = 0;
    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let mut db = db.with_bytecode(caller_address, bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());

    let expected_contract_call_result = 0_u8.into(); //Call failed

    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap().result;
    assert!(result.is_success());

    let res_bytes: &[u8] = result.output().unwrap();

    let contract_call_result = BigUint::from_bytes_be(&res_bytes[..32]);
    let caller_balance_result = BigUint::from_bytes_be(&res_bytes[32..]);

    assert_eq!(contract_call_result, expected_contract_call_result);
    assert_eq!(caller_balance_result, caller_balance.into());
}

#[test]
fn call_gas_check_with_value_zero_args_return_and_non_empty_callee() {
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

    let callee_memory_expansion_cost = 6; //Memory expansion of size 32
    let callee_gas_cost = gas_cost::PUSHN + gas_cost::PUSH0 * 3 + callee_memory_expansion_cost;

    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let db = db.with_bytecode(callee_address, bytecode);

    let gas = callee_gas_cost as u8;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 64_u8;
    let ret_offset = 0_u8;
    let ret_size = 32_u8;

    /*
    This test will check for the exact ammount of gas needed to perform a successful call without a value transfer
    Since the gas sent to the callee has to be 1/64 th of the remaining gas, we have to add operations to
    the caller to burn the extra gas that we have to add in order for the calle to receive the needed ammount

    (needed_gas - caller_gas_cost) * 1/64 >= callee_gas_cost
    needed_gas = caller_gas_cost + added_caller_gas_cost + callee_gas_cost

    For the proposed set of operations, we have that:

    callee_gas_cost = 15
    caller_gas_cost = 144

    => needed_gas >= 945
    => added_caller_gas_cost >= 472.5

    So, we have to add extra operations to caller in order to burn at least 473 of gas
    */

    let caller_address = Address::from_low_u64_be(4040);
    let mut caller_ops = vec![
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
        Operation::Push((1_u8, BigUint::from(value))), //Value
        Operation::Push((16_u8, BigUint::from_bytes_be(callee_address.as_bytes()))), //Address
        Operation::Push((1_u8, BigUint::from(gas))),  //Gas
        Operation::Call,
    ];

    let additional_caller_ops = 473;
    caller_ops.extend(vec![Operation::Push0; additional_caller_ops]);

    let caller_memory_expansion_cost = 6 * 2; // Memory expansion of size 64
    let caller_call_cost = 100; //EVM Codes gas calculator (memory already expanded)
    let caller_gas_cost =
        gas_cost::PUSHN * 10 + gas_cost::PUSH0 + caller_memory_expansion_cost + caller_call_cost;
    let added_caller_gas_cost = gas_cost::PUSH0 * additional_caller_ops as i64;

    let needed_gas = caller_gas_cost + added_caller_gas_cost + callee_gas_cost;

    let caller_balance: u8 = 0;
    let program = Program::from(caller_ops);
    let bytecode = Bytecode::from(program.to_bytecode());
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(caller_address);
    env.tx.caller = caller_address;
    let mut db = db.with_bytecode(caller_address, bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());

    run_program_assert_gas_exact_with_db(env, db, needed_gas as _);
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
    let db = db.with_bytecode(callee_address, bytecode);

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
    let db = db.with_bytecode(caller_address, bytecode);

    run_program_assert_bytes_result(env, db, expected_result);
}

#[test]
fn call_check_stack_underflow() {
    let program = vec![Operation::Call];
    let (env, db) = default_env_and_db_setup(program);
    run_program_assert_halt(env, db);
}

#[test]
fn call_gas_check_with_value_and_empty_account() {
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
    let db = db.with_bytecode(callee_address, bytecode);

    let gas = 255_u8;
    let value = 3_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

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
    ];

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
    let mut db = db.with_bytecode(caller_address, bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());

    run_program_assert_gas_exact_with_db(env, db, needed_gas as _);
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
    db = db.with_bytecode(address, bytecode);

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
    db = db.with_bytecode(address, bytecode);

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
    db = db.with_bytecode(address, bytecode);
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
    let db = Db::default().with_bytecode(callee_address, bytecode);

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
    let db = db.with_bytecode(caller_address, bytecode);

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
    let db = Db::default().with_bytecode(caller_address, bytecode);

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
    let db = Db::default().with_bytecode(callee_address, bytecode);

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
    let db = db.with_bytecode(caller_address, bytecode);

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
    let db = Db::default().with_bytecode(caller_address, bytecode);

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
    let db = Db::default().with_bytecode(callee_address, bytecode);

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
    let db = db.with_bytecode(caller_address, bytecode);

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
    let db = Db::default().with_bytecode(callee_address, bytecode);

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
    let db = db.with_bytecode(caller_address, bytecode);

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
    let db = Db::default().with_bytecode(callee_address, bytecode);

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
    let db = db.with_bytecode(caller_address, bytecode);

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

//TODO: Add CREATE, CREATE2, SELFDESTRUCT
#[ignore]
#[rstest]
//SSTORE
#[case(
        vec![
            Operation::Push((1_u8, 1_u8.into())),
            Operation::Push((1_u8, 1_u8.into())),
            Operation::Sstore,
        ]
)]
//CALL with value > 0
#[case(
        vec![
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
        ],
)]
//LOG0
#[case(
        vec![
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Log(0),
        ]
)]
//LOG1
#[case(
        vec![
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Log(0),
        ]
)]
//LOG2
#[case(
        vec![
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Log(0),
        ]
)]
//LOG3
#[case(
        vec![
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Log(0),
        ]
)]
//LOG4
#[case(
        vec![
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Push((1_u8, 0_u8.into())),
            Operation::Log(0),
        ]
)]
fn staticcall_state_modifying_revert_with_callee_ops(#[case] callee_ops: Vec<Operation>) {
    let caller_address = Address::from_low_u64_be(4040);
    let db = Db::new();
    let program = Program::from(callee_ops);
    let (callee_address, bytecode) = (
        Address::from_low_u64_be(8080),
        Bytecode::from(program.to_bytecode()),
    );
    let callee_balance = 100_u8;
    let mut db = db.with_bytecode(callee_address, bytecode);
    db.set_account(callee_address, 0, callee_balance.into(), Default::default());

    let gas = 1_000_000_u32;
    let value = 0_u8;
    let args_offset = 0_u8;
    let args_size = 0_u8;
    let ret_offset = 0_u8;
    let ret_size = 0_u8;

    let mut caller_ops = vec![
        Operation::Push((1_u8, BigUint::from(ret_size))), //Ret size
        Operation::Push((1_u8, BigUint::from(ret_offset))), //Ret offset
        Operation::Push((1_u8, BigUint::from(args_size))), //Args size
        Operation::Push((1_u8, BigUint::from(args_offset))), //Args offset
        Operation::Push((1_u8, BigUint::from(value))),    //Value
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
    let mut db = db.with_bytecode(caller_address, bytecode);
    db.set_account(caller_address, 0, caller_balance.into(), Default::default());

    let expected_result = 0_u8.into();

    run_program_assert_num_result(env, db, expected_result);
}

#[test]
fn staticcall_test() {
    let mut operations = vec![Operation::StaticCall];
    append_return_result_operations(&mut operations);
    let (env, db) = default_env_and_db_setup(operations);
    let expected_result = 1_u8.into(); // Context is static

    run_program_assert_num_result(env, db, expected_result);
}
