use std::path::PathBuf;

use evm_mlir::{
    constants::MAIN_ENTRYPOINT,
    context::Context,
    program::Program,
    syscall::{register_syscalls, MainFunc, SyscallContext},
};
use melior::ExecutionEngine;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("No path provided").as_str();
    let bytecode = std::fs::read(path).expect("Could not read file");
    let program = Program::from_bytecode(&bytecode);

    // This is for intermediate files
    let output_file = PathBuf::from("output");

    let context = Context::new();
    let module = context
        .compile(&program, &output_file)
        .expect("failed to compile program");

    let engine = ExecutionEngine::new(module.module(), 0, &[], false);
    register_syscalls(&engine);

    let function_name = format!("_mlir_ciface_{MAIN_ENTRYPOINT}");
    let fptr = engine.lookup(&function_name);
    let main_fn: MainFunc = unsafe { std::mem::transmute(fptr) };

    let mut context = SyscallContext::default();
    let initial_gas = 1000;

    main_fn(&mut context, initial_gas);
}
