use std::path::PathBuf;

use evm_mlir::{
    context::Context,
    executor::Executor,
    program::{OpcodeParseError, Program},
    syscall::SyscallContext,
};

fn log_failed_opcodes(failed_opcodes: Vec<OpcodeParseError>) {
    eprintln!("Failed opcodes: {:#?}", failed_opcodes);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("No path provided").as_str();
    let bytecode = std::fs::read(path).expect("Could not read file");
    let program = Program::from_bytecode(&bytecode);

    if let Err(failed_opcodes) = program {
        log_failed_opcodes(failed_opcodes);
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
}
