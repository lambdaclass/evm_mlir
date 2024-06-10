use std::path::PathBuf;

use evm_mlir::{context::Context, executor::Executor, program::Program, syscall::SyscallContext};
// use num_bigint::BigUint;
// use evm_mlir::program::Operation;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("No path provided").as_str();
    let bytecode = std::fs::read(path).expect("Could not read file");
    let program = Program::from_bytecode(&bytecode);

    if let Err(err) = program {
        eprintln!("{:#?}", err);
        return;
    }

    // This is for intermediate files
    let output_file = PathBuf::from("output");

    let context = Context::new();
    let module = context
        .compile(&program.unwrap(), &output_file)
        .expect("failed to compile program");

    let executor = Executor::new(&module);

    let mut context = SyscallContext::default();
    let initial_gas = 1000;

    let result = executor.execute(&mut context, initial_gas);

    println!("Execution result: {result}");

    // let operations = vec![
    //     Operation::Push((1, BigUint::from(5_u8))),
    //     Operation::Push((1, BigUint::from(0_u8))),
    //     Operation::Push((1, BigUint::from(0_u8))),
    //     Operation::CallDataCopy,
    // ];
    // let program = Program::from(operations);
    // let output_file = PathBuf::from("output");

    // let context = Context::new();
    // let module = context
    //     .compile(&program, &output_file)
    //     .expect("failed to compile program");

    // let executor = Executor::new(&module);

    // let mut context = SyscallContext::default();

    // let mut vec: Vec<u8> = vec![];
    // for i in 0..10 {
    //     vec.push(i as u8);
    // }

    // context.env.tx.calldata = vec;

    // let initial_gas = 1000;

    // let result = executor.execute(&mut context, initial_gas);
    // for byte in context.memory {
    //     println!("byte = {:X}", byte);
    // }
    // println!("Execution result: {result}");
}
