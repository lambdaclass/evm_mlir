use std::path::PathBuf;

use evm_mlir::{
    context::Context,
    executor::Executor,
    program::{Operation, Program},
    syscall::SyscallContext,
};

fn main() {
    // let args: Vec<String> = std::env::args().collect();
    // let path = args.get(1).expect("No path provided").as_str();
    // let bytecode = std::fs::read(path).expect("Could not read file");
    // let program = Program::from_bytecode(&bytecode);

    // if let Err(err) = program {
    //     eprintln!("{:#?}", err);
    //     return;
    // }

    // This is for intermediate files
    let output_file = PathBuf::from("output");
    use num_bigint::BigUint;
    let size = 7_u8;
    let offset = 0_u8;
    let dest_offset = 0_u8;
    let program: Vec<Operation> = vec![
        Operation::Push((1_u8, BigUint::from(size))),
        Operation::Push((1_u8, BigUint::from(offset))),
        Operation::Push((1_u8, BigUint::from(dest_offset))),
        Operation::Codecopy,
        //Operation::Push((1_u8, BigUint::from(dest_offset))),
        //Operation::Mload,
    ];
    let mut bytecode = vec![];
    for op in program.clone() {
        let bytes = op.to_bytecode();
        for byte in bytes {
            bytecode.push(byte);
        }
    }
    let program: Program = program.into();

    let context = Context::new();
    let module = context
        .compile(&program, &output_file)
        .expect("failed to compile program");

    let executor = Executor::new(&module);

    let mut context = SyscallContext::default();
    println!("bytecode = {:X?}", bytecode.clone());
    context.program = bytecode;

    let initial_gas = 1000;

    let result = executor.execute(&mut context, initial_gas);

    println!("Execution result: {result}");
    println!("Memory: ");
    for byte in context.memory {
        println!("byte = {:X}", byte);
    }
}
