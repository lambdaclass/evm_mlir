use evm_mlir::{context::Context, executor::Executor, program::Program, syscall::SyscallContext};
use revm::{
    db::BenchmarkDB,
    primitives::{address, bytes, Bytecode, TransactTo},
    Evm,
};
use std::path::PathBuf;

pub fn run_with_evm_mlir(program: &str, runs: usize) {
    let bytes = hex::decode(program).unwrap();
    let program = Program::from_bytecode(&bytes);

    // This is for intermediate files
    let output_file = PathBuf::from("output");

    let context = Context::new();
    let module = context
        .compile(&program, &output_file)
        .expect("failed to compile program");

    let executor = Executor::new(&module);
    let mut context = SyscallContext::default();
    let initial_gas = 999_999_999;

    for _ in 0..runs {
        let _result = executor.execute(&mut context, initial_gas);
        assert!(context.get_result().is_success());
    }
}

pub fn run_with_revm(program: &str, runs: usize) {
    let raw = Bytecode::new_raw(program.to_owned().into());
    let mut evm = Evm::builder()
        .with_db(BenchmarkDB::new_bytecode(raw))
        .modify_tx_env(|tx| {
            tx.caller = address!("1000000000000000000000000000000000000000");
            tx.transact_to = TransactTo::Call(address!("0000000000000000000000000000000000000000"));
            tx.data = bytes!("");
        })
        .build();

    for _ in 0..runs {
        let result = evm.transact().unwrap();
        assert!(result.result.is_success());
    }
}
