use std::{ffi::c_void, path::PathBuf};

use evm_mlir::{
    compile_binary,
    context::Context,
    program::Program,
    syscall_handler::{MainFunc, SyscallContext, SyscallHandler},
};
use melior::ExecutionEngine;

fn main() {
    // TODO: clean this up
    //  - move context ops to function
    //  - improve main code
    //  - implement RETURN opcode
    //  - try doing the same by registering global symbols
    // TODO: read from stdio?
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

    unsafe {
        engine.register_symbol(
            "write_result",
            SyscallContext::wrap_write_result as *const fn(*mut c_void, *const u8, u64) as *mut (),
        )
    };

    let function_name = "_mlir_ciface_main";
    let fptr = engine.lookup(function_name);
    let main_fn: MainFunc = unsafe { std::mem::transmute(fptr) };

    let mut syscall_handler = SyscallHandler { result: vec![] };
    let mut context_ptr = SyscallContext::new(&mut syscall_handler);

    main_fn(&mut context_ptr);

    println!("Result: {:?}", syscall_handler.result[0]);

    assert_eq!(syscall_handler.result[0], 42);
}

#[allow(dead_code)]
fn compile_from_file() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("No path provided").as_str();
    let bytecode = std::fs::read(path).expect("Could not read file");
    let program = Program::from_bytecode(&bytecode);
    let output_file = "output";

    compile_binary(&program, output_file).unwrap();
    println!("Done!");
    println!("Program was compiled in {output_file}");
}
