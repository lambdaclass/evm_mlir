use std::path::PathBuf;

use evm_mlir::{
    context::Context,
    db::Db,
    env::Env,
    executor::{Executor, OptLevel},
    program::Program,
    syscall::SyscallContext,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("No path provided").as_str();
    let opt_level = match args.get(2).map(String::as_str) {
        None | Some("2") => OptLevel::Default,
        Some("0") => OptLevel::None,
        Some("1") => OptLevel::Less,
        Some("3") => OptLevel::Aggressive,
        _ => panic!("Invalid optimization level"),
    };
    let bytecode = std::fs::read(path).expect("Could not read file");
    let program = Program::from_bytecode(&bytecode);

    // This is for intermediate files
    let output_file = PathBuf::from("output");

    let context = Context::new();
    let module = context
        .compile(&program, &output_file)
        .expect("failed to compile program");

    let executor = Executor::new(&module, opt_level);

    let env = Env::default();
    let mut db = Db::default();
    let mut context = SyscallContext::new(env, &mut db, Default::default());

    let initial_gas = 1000;

    let result = executor.execute(&mut context, initial_gas);
    println!("Execution result: {result}");
}
