use std::path::PathBuf;

use evm_mlir::{
    context::Context,
    program::{Operation, Program},
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

    // unsafe {
    //     engine.register_symbol(
    //         "cairo_native__libfunc__debug__print",
    //         cairo_native_runtime::cairo_native__libfunc__debug__print
    //             as *const fn(i32, *const [u8; 32], usize) -> i32 as *mut (),
    //     );
    // }

    // let fptr = engine.lookup("main") as *mut c_void;
    let mut result = 0_i64;
    unsafe { engine.invoke_packed("main", &mut [&mut result as *mut i64 as *mut ()]) }.unwrap();

    assert_eq!(result, (a + b).try_into().unwrap());

    assert!(output_file.exists(), "output file does not exist");
}
