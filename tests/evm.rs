use std::str::FromStr;

use evm_mlir::{
    constants::gas_cost,
    db::{Bytecode, Db},
    env::TransactTo,
    primitives::{Address, Bytes, U256 as EU256},
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
    let result_data = BigUint::from_bytes_be(result.output().unwrap());
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

fn run_program_assert_gas_exact(operations: Vec<Operation>, env: Env, needed_gas: u64) {
    let address = match env.tx.transact_to {
        TransactTo::Call(a) => a,
        TransactTo::Create => Address::zero(),
    };
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
    env.tx.gas_limit = 999_999;
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
    env.tx.gas_limit = 999_999;
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

    db.update_account(address, 0, balance);

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

    db.update_account(address, 0, balance);

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
    db.update_account(contract_address, 0, contract_balance.into());
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
    db.update_account(contract_address, 0, contract_balance.into());
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
