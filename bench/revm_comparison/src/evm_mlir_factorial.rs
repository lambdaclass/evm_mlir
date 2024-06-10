use hex::decode;
use std::path::PathBuf;

use evm_mlir::{context::Context, executor::Executor, program::Program, syscall::SyscallContext};

fn main() {
    const PROGRAM: &str = "7f0000000000000000000000000000000000000000000000000000000000000080600260025b8215603b57906001018091029160019003916025565b9150505f5260205ff3";
    const RUNS: usize = 100000;
    let bytes = decode(PROGRAM).unwrap();
    let program = Program::from_bytecode(&bytes);

    // This is for intermediate files
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
    }
}
