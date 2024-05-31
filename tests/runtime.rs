#![allow(dead_code)]
use std::path::PathBuf;

use evm_mlir::{
    context::Context,
    program::{Operation, Program},
    syscall_handler::{MainFunc, SyscallHandler, SyscallHandlerCallbacks},
};
use melior::ExecutionEngine;
use num_bigint::BigUint;

#[test]
fn runtime_test() {
    let (a, b) = (BigUint::from(11_u8), BigUint::from(31_u8));

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Add,
    ];
    let program = Program::from(program);
    // This is for intermediate files
    let output_file = PathBuf::from("output");

    let context = Context::new();
    let module = context
        .compile(&program, &output_file)
        .expect("failed to compile program");

    let engine = ExecutionEngine::new(module.module(), 0, &[], false);

    // See: fn cairo_native__libfunc__debug__print
    // unsafe {
    //     engine.register_symbol(
    //         "cairo_native__libfunc__debug__print",
    //         cairo_native_runtime::cairo_native__libfunc__debug__print
    //             as *const fn(i32, *const [u8; 32], usize) -> i32 as *mut (),
    //     );
    // }

    let mut handler = SyscallHandler { counter: 41 };

    let function_name = "_mlir_ciface_main";
    let fptr = engine.lookup(function_name);
    let main_fn: MainFunc = unsafe { std::mem::transmute(fptr) };

    let callbacks = SyscallHandlerCallbacks::new(&mut handler);

    let result = main_fn(&callbacks);

    assert_eq!(result, (a + b).try_into().unwrap());

    assert_eq!(handler.counter, 42);
}
