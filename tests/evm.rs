use evm_mlir::{
    constants::gas_cost,
    db::{Bytecode, Db},
    env::TransactTo,
    primitives::{Address, Bytes, U256 as EU256},
    program::{Operation, Program},
    syscall::{Log, U256},
    Env, Evm,
};
use num_bigint::BigUint;
use std::str::FromStr;

fn run_program_assert_result(
    mut operations: Vec<Operation>,
    mut env: Env,
    expected_result: BigUint,
) {
    operations.extend([
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ]);
    env.tx.gas_limit = 999_999;
    let program = Program::from(operations);
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);
    let result = evm.transact();
    assert!(&result.is_success());
    let result_data = BigUint::from_bytes_be(result.return_data().unwrap());
    assert_eq!(result_data, expected_result);
}

fn run_program_assert_halt(operations: Vec<Operation>, mut env: Env) {
    let program = Program::from(operations);
    env.tx.gas_limit = 999_999;
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact();
    assert!(result.is_halt());
}

fn run_program_assert_gas_exact(operations: Vec<Operation>, env: Env, needed_gas: u64) {
    //Ok run
    let program = Program::from(operations.clone());
    let mut env_success = env.clone();
    env_success.tx.gas_limit = needed_gas;
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env_success.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env_success, db);

    let result = evm.transact();
    assert!(result.is_success());
    //Halt run
    let program = Program::from(operations.clone());
    let mut env_halt = env.clone();
    env_halt.tx.gas_limit = needed_gas - 1;
    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.to_bytecode()),
    );
    env_halt.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env_halt, db);

    let result = evm.transact();
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

    let result = evm.transact();

    assert!(&result.is_success());
    let number = BigUint::from_bytes_be(result.return_data().unwrap());
    assert_eq!(number, 55_u32.into());
}

#[test]
fn test_opcode_origin() {
    let operations = vec![Operation::Origin];
    let mut env = Env::default();
    let caller = Address::from_str("0x9bbfed6889322e016e0a02ee459d306fc19545d8").unwrap();
    env.tx.caller = caller;
    let caller_bytes = &caller.to_fixed_bytes();
    //We extend the result to be 32 bytes long.
    let expected_result: [u8; 32] = [&[0u8; 12], &caller_bytes[0..20]]
        .concat()
        .try_into()
        .unwrap();
    run_program_assert_result(operations, env, BigUint::from_bytes_be(&expected_result));
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
    let env = Env::default();
    run_program_assert_halt(program, env);
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

    let result = evm.transact();

    assert!(&result.is_success());
    let calldata_slice = result.return_data().unwrap();
    let mut expected_result = [0_u8; 32];
    expected_result[31] = 1;
    assert_eq!(calldata_slice, expected_result);
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

    let result = evm.transact();

    assert!(&result.is_success());
    let calldata_slice = result.return_data().unwrap();
    let mut expected_result = [0_u8; 32];
    expected_result[30] = 1;
    assert_eq!(calldata_slice, expected_result);
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

    let result = evm.transact();

    assert!(&result.is_success());
    let calldata_slice = result.return_data().unwrap();
    let expected_result = [0_u8; 32];
    assert_eq!(calldata_slice, expected_result);
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

    let result = evm.transact();

    assert!(&result.is_success());
    let logs = result.return_logs().unwrap();
    let expected_logs: Vec<Log> = vec![Log {
        data: [0xff_u8; 32].into(),
        topics: vec![],
    }];
    assert_eq!(logs.to_owned(), expected_logs);
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

    let result = evm.transact();

    assert!(&result.is_success());
    let logs = result.return_logs().unwrap();
    let expected_logs: Vec<Log> = vec![Log {
        data: [0xff_u8; 32].into(),
        topics: vec![U256 { lo: 1, hi: 0 }],
    }];
    assert_eq!(logs.to_owned(), expected_logs);
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

    let result = evm.transact();

    assert!(&result.is_success());
    let logs = result.return_logs().unwrap();
    let expected_logs: Vec<Log> = vec![Log {
        data: [0xff_u8; 32].into(),
        topics: vec![U256 { lo: 1, hi: 0 }, U256 { lo: 2, hi: 0 }],
    }];
    assert_eq!(logs.to_owned(), expected_logs);
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

    let result = evm.transact();

    assert!(&result.is_success());
    let logs = result.return_logs().unwrap();
    let expected_logs: Vec<Log> = vec![Log {
        data: [0xff_u8; 32].into(),
        topics: vec![
            U256 { lo: 1, hi: 0 },
            U256 { lo: 2, hi: 0 },
            U256 { lo: 3, hi: 0 },
        ],
    }];
    assert_eq!(logs.to_owned(), expected_logs);
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

    let result = evm.transact();

    assert!(&result.is_success());
    let logs = result.return_logs().unwrap();
    let expected_logs: Vec<Log> = vec![Log {
        data: [0xff_u8; 32].into(),
        topics: vec![
            U256 { lo: 1, hi: 0 },
            U256 { lo: 2, hi: 0 },
            U256 { lo: 3, hi: 0 },
            U256 { lo: 4, hi: 0 },
        ],
    }];
    assert_eq!(logs.to_owned(), expected_logs);
}

#[test]
fn callvalue_happy_path() {
    let callvalue: u32 = 1500;
    let operations = vec![Operation::Callvalue];
    let mut env = Env::default();
    env.tx.value = EU256::from(callvalue);

    let expected_result = BigUint::from(callvalue);

    run_program_assert_result(operations, env, expected_result);
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
    let env = Env::default();
    run_program_assert_halt(program, env);
}

#[test]
fn block_number_check() {
    let program = vec![Operation::Number];
    let mut env = Env::default();
    let result = BigUint::from(2147483639_u32);

    env.block.number = ethereum_types::U256::from(2147483639);

    run_program_assert_result(program, env, result);
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
    let env = Env::default();

    program.push(Operation::Number);
    run_program_assert_halt(program, env);
}

#[test]
fn gasprice_happy_path() {
    let gas_price: u32 = 33192;
    let operations = vec![Operation::Gasprice];
    let mut env = Env::default();
    env.tx.gas_price = EU256::from(gas_price);

    let expected_result = BigUint::from(gas_price);

    run_program_assert_result(operations, env, expected_result);
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
    let env = Env::default();
    run_program_assert_halt(program, env);
}

#[test]
fn chainid_happy_path() {
    let chainid: u64 = 1333;
    let operations = vec![Operation::Chainid];
    let mut env = Env::default();
    env.cfg.chain_id = chainid;
    let expected_result = BigUint::from(chainid);
    run_program_assert_result(operations, env, expected_result);
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
    let env = Env::default();
    run_program_assert_halt(program, env);
}

#[test]
fn balance_with_unexisting_account() {
    let operations = vec![
        Operation::Push((20_u8, BigUint::from(1_u8))),
        Operation::Balance,
    ];
    let env = Env::default();

    run_program_assert_result(operations, env, BigUint::from(0_u8));
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

    let result = evm.transact();

    assert!(&result.is_success());
    let result = result.return_data().unwrap();
    let expected_result = BigUint::from(123456_u32);
    assert_eq!(BigUint::from_bytes_be(result), expected_result);
}

#[test]
fn balance_with_stack_underflow() {
    let program = vec![Operation::Balance];
    let env = Env::default();

    run_program_assert_halt(program, env);
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
