use std::path::PathBuf;

use evm_mlir::{
    context::Context,
    executor::Executor,
    program::{Operation, Program},
    syscall::SyscallContext,
};
use num_bigint::BigUint;

fn main() {
    const RUNS: usize = 100000;
    // let args: Vec<String> = std::env::args().collect();
    let path = "factorial_1024.bytecode";
    let bytecode = std::fs::read(path).expect("Could not read file");
    let program = Program::from_bytecode(&bytecode);

    // // This is for intermediate files
    let output_file = PathBuf::from("output");

    let context = Context::new();
    let module = context
        .compile(&program, &output_file)
        .expect("failed to compile program");

    let executor = Executor::new(&module);
    let mut context = SyscallContext::default();
    let initial_gas = 999_999_999;

    for _ in 0..RUNS {
        let _result = executor.execute(&mut context, initial_gas);
        assert!(context.get_result().is_success());
        // dbg!(context.get_result());
    }
}
