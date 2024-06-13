use std::path::PathBuf;

use evm_mlir::{
    context::Context, db::Db, env::Env, executor::Executor, program::Program,
    syscall::SyscallContext,
};

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

    let executor = Executor::new(&module, Default::default());

    let env = Env::default();
    let mut db = Db::default();
    let mut context = SyscallContext::new(env, &mut db);

    let initial_gas = 1000;

    let result = executor.execute(&mut context, initial_gas);
    println!("Execution result: {result}");
}
