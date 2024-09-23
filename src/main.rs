use std::path::PathBuf;

use evm_mlir::{
    context::{Context, Session},
    db::Db,
    env::Env,
    executor::{Executor, OptLevel},
    journal::Journal,
    program::Program,
    syscall::SyscallContext,
};
use evm_mlir::program::Operation::{Push, Push0, Sstore, CalldataLoad, Jump, Msize, Mcopy, Stop, Jumpdest};
use num_bigint::BigUint;

pub fn custom_program() -> Program {
    Program { 
        operations: [Push((1, BigUint::from_bytes_be(&[1]))), Push0, Sstore, Push((1, BigUint::from_bytes_be(&[17]))), Push((1, BigUint::from_bytes_be(&[64]))), CalldataLoad, Push((1, BigUint::from_bytes_be(&[32]))), CalldataLoad, Push0, CalldataLoad, Push((1, BigUint::from_bytes_be(&[22]))), Jump, Jumpdest { pc: 17 }, Msize, Push0, Sstore, Stop, Jumpdest { pc: 22 }, Mcopy, Jump].to_vec(), 
        code_size: 25 
    }
}

// fn main() {
//     // let args: Vec<String> = std::env::args().collect();
//     // let path = args.get(1).expect("No path provided").as_str();
//     // let opt_level = match args.get(2).map(String::as_str) {
//     //     None | Some("2") => OptLevel::Default,
//     //     Some("0") => OptLevel::None,
//     //     Some("1") => OptLevel::Less,
//     //     Some("3") => OptLevel::Aggressive,
//     //     _ => panic!("Invalid optimization level"),
//     // };
//     // let bytecode = std::fs::read(path).expect("Could not read file");
//     //let program = Program::from_bytecode(&bytecode);

//     let program = custom_program();

//     let session = Session {
//         raw_mlir_path: Some(PathBuf::from("output")),
//         ..Default::default()
//     };

//     let context = Context::new();
//     let module = context
//         .compile(&program, session)
//         .expect("failed to compile program");

//     let env = Env::default();
//     let initial_gas = env.tx.gas_limit;
//     let mut db = Db::default();
//     let journal = Journal::new(&mut db);
//     let mut context = SyscallContext::new(env, journal, Default::default(), initial_gas);
//     let executor = Executor::new(&module, &context, OptLevel::Aggressive);

//     let result = executor.execute(&mut context, initial_gas);
//     println!("Execution result: {result}");
// }

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

    let session = Session {
        raw_mlir_path: Some(PathBuf::from("output")),
        ..Default::default()
    };

    let context = Context::new();
    let module = context
        .compile(&program, session)
        .expect("failed to compile program");

    let env = Env::default();
    let initial_gas = env.tx.gas_limit;
    let mut db = Db::default();
    let journal = Journal::new(&mut db);
    let mut context = SyscallContext::new(env, journal, Default::default(), initial_gas);
    let executor = Executor::new(&module, &context, opt_level);

    let result = executor.execute(&mut context, initial_gas);
    println!("Execution result: {result}");
}
