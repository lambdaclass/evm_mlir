use std::path::PathBuf;

use evm_mlir::{
    compile_binary,
    context::Context,
    program::Program,
    syscall_handler::{MainFunc, SyscallHandler, SyscallHandlerCallbacks},
};
use melior::ExecutionEngine;

fn main() {
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

    // TODO: check this
    // See: fn cairo_native__libfunc__debug__print
    // unsafe {
    //     engine.register_symbol(
    //         "cairo_native__libfunc__debug__print",
    //         cairo_native_runtime::cairo_native__libfunc__debug__print
    //             as *const fn(i32, *const [u8; 32], usize) -> i32 as *mut (),
    //     );
    // }

    // let mut handler = SyscallHandler { counter: 41 };

    let function_name = "_mlir_ciface_main";
    let fptr = engine.lookup(function_name);
    let main_fn: MainFunc = unsafe { std::mem::transmute(fptr) };

    // TODO: Pass *return_ptr and *size
    // inside the function allocate memory for the result via a syscall
    // and write the result to the return_ptr.
    // Alt: allocate memory and save the return value in the syscall handler.
    let mut syscall_handler = SyscallHandler { result: vec![] };
    let mut callbacks = SyscallHandlerCallbacks::new(&mut syscall_handler);

    dbg!("gagsagasga");
    main_fn(&mut callbacks);

    dbg!("gagsagasga");

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
