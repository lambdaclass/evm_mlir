use std::path::PathBuf;

use evm_mlir::{
    constants::MAIN_ENTRYPOINT,
    context::Context,
    program::Program,
    syscall::{register_syscalls, MainFunc, SyscallContext},
};
use melior::ExecutionEngine;

fn main() {
    use evm_mlir::program::Operation;
    // let args: Vec<String> = std::env::args().collect();
    // let path = args.get(1).expect("No path provided").as_str();
    // let bytecode = std::fs::read(path).expect("Could not read file");
    let value: [u8; 32] = [0xaa; 32];
    let offset = 0_u8;
    let value2: [u8; 32] = [0xbb; 32];
    let offset2 = 1_u8;
    use num_bigint::BigUint;
    let program: Program = vec![
        Operation::Push(BigUint::from_bytes_be(&value)),
        Operation::Push(BigUint::from(offset)),
        Operation::Mstore,
        Operation::Push(BigUint::from_bytes_be(&value2)),
        Operation::Push(BigUint::from(offset2)),
        Operation::Mstore8,
    ]
    .into();
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

    main_fn(&mut context);
    let memory = context.memory;
    for (i, byte) in memory.into_iter().enumerate() {
        println!("byte {i} = {:X}", byte);
    }
}
