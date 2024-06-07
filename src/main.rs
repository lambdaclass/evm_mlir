use std::path::PathBuf;

use evm_mlir::{
    context::Context,
    executor::Executor,
    program::{Operation, Program},
    syscall::SyscallContext,
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

fn main() {
    const RUNS: usize = 10000;
    // let args: Vec<String> = std::env::args().collect();
    // let path = args.get(1).expect("No path provided").as_str();
    // let bytecode = std::fs::read(path).expect("Could not read file");
    // let program = Program::from_bytecode(&bytecode);
    let p = get_fibonacci_program(1000);
    let x: Vec<u8> = p.iter().flat_map(Operation::to_bytecode).collect();
    std::fs::write("fibonacci_1000.bytecode", x).unwrap();
    let program = Program::from(get_fibonacci_program(1000));

    // // This is for intermediate files
    let output_file = PathBuf::from("output");

    let context = Context::new();
    let module = context
        .compile(&program, &output_file)
        .expect("failed to compile program");

    let executor = Executor::new(&module);
    let mut context = SyscallContext::default();
    let initial_gas = 999_999;

    for _ in 0..RUNS {
        let _result = executor.execute(&mut context, initial_gas);
        assert!(context.get_result().is_success());
    }
}
