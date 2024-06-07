use evm_mlir::{
    program::{Operation, Program},
    Env, Evm,
};
use num_bigint::BigUint;

fn get_fibonacci_program(n: u64) -> Vec<Operation> {
    assert!(n > 0, "n must be greater than 0");

    let main_loop_pc = 34;
    let end_pc = 55;
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

    let bytecode: Vec<u8> = operations.iter().flat_map(Operation::to_bytecode).collect();
    std::fs::write("fibonacci.bytecode", bytecode).unwrap();

    let program = Program::from(operations);
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;

    let evm = Evm::new(env, program);

    let result = evm.transact();

    assert!(&result.is_success());
    let number = BigUint::from_bytes_be(result.return_data().unwrap());
    println!("{number}");
}
