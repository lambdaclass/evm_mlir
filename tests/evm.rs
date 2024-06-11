use evm_mlir::{
    program::{Operation, Program},
    Env, Evm,
};
use num_bigint::BigUint;

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

    let evm = Evm::new(env, program);

    let result = evm.transact();

    assert!(&result.is_success());
    let number = BigUint::from_bytes_be(result.return_data().unwrap());
    assert_eq!(number, 55_u32.into());
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
    env.tx.calldata = [0x00; 64].into();
    env.tx.calldata[31] = 1;
    let evm = Evm::new(env, program);

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
    env.tx.calldata = [0x00; 32].into();
    env.tx.calldata[31] = 1;
    let evm = Evm::new(env, program);

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
    env.tx.calldata = [0xff; 32].into();
    let evm = Evm::new(env, program);

    let result = evm.transact();

    assert!(&result.is_success());
    let calldata_slice = result.return_data().unwrap();
    let expected_result = [0_u8; 32];
    assert_eq!(calldata_slice, expected_result);
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
    env.tx.calldata = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    env.tx.gas_limit = 1000;
    let evm = Evm::new(env, program);
    let result = evm.transact();

    //Test that the memory is correctly copied
    let correct_memory = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    let return_data = result.return_data().unwrap();
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
    env.tx.calldata = vec![0, 1, 2, 3, 4];
    env.tx.gas_limit = 1000;
    let evm = Evm::new(env, program);
    let result = evm.transact();

    //Test that the memory is correctly copied
    let correct_memory = vec![0, 1, 2, 3, 4, 0, 0, 0, 0, 0];
    let return_data = result.return_data().unwrap();
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
    env.tx.calldata = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    env.tx.gas_limit = 1000;
    let evm = Evm::new(env, program);
    let result = evm.transact();

    //Test that the memory is correctly copied
    let correct_memory = vec![1, 2, 3, 4, 5];
    let return_data = result.return_data().unwrap();
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
    env.tx.calldata = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    env.tx.gas_limit = 1000;
    let evm = Evm::new(env, program);

    let result = evm.transact();

    //Test that the memory is correctly copied
    let correct_memory = vec![0, 0, 1, 2, 3, 4, 5, 6, 7, 8];
    let return_data = result.return_data().unwrap();
    assert_eq!(return_data, correct_memory);
}
