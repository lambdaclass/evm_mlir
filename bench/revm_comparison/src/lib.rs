use evm_mlir::{
    context::Context, executor::Executor, program::Program, syscall::SyscallContext, Env,
};
use revm::{
    db::BenchmarkDB,
    primitives::{address, Bytecode, TransactTo},
    Evm,
};
use std::path::PathBuf;

pub fn run_with_evm_mlir(program: &str, runs: usize, parameter: u32) {
    let bytes = hex::decode(program).unwrap();
    let program = Program::from_bytecode(&bytes).unwrap();

    // This is for intermediate files
    let output_file = PathBuf::from("output");

    let context = Context::new();
    let module = context
        .compile(&program, &output_file)
        .expect("failed to compile program");

    let executor = Executor::new(&module);
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.calldata = [0x00; 32].into();
    env.tx.calldata[28..32].copy_from_slice(&parameter.to_be_bytes());
    let mut context = SyscallContext::with_env(env);
    let initial_gas = 999_999_999;

    for _ in 0..runs - 1 {
        let _result = executor.execute(&mut context, initial_gas);
        assert!(context.get_result().is_success());
    }
    executor.execute(&mut context, initial_gas);
    assert!(context.get_result().is_success());

    println!(
        "\t0x{}",
        hex::encode(context.get_result().return_data().unwrap())
    );
}

pub fn run_with_revm(program: &str, runs: usize, parameter: u32) {
    let bytes = hex::decode(program).unwrap();
    let raw = Bytecode::new_raw(bytes.into());
    let mut calldata = [0; 32];
    calldata[28..32].copy_from_slice(&parameter.to_be_bytes());
    let mut evm = Evm::builder()
        .with_db(BenchmarkDB::new_bytecode(raw))
        .modify_tx_env(|tx| {
            tx.caller = address!("1000000000000000000000000000000000000000");
            tx.transact_to = TransactTo::Call(address!("0000000000000000000000000000000000000000"));
            tx.data = calldata.into();
        })
        .build();

    for _ in 0..runs - 1 {
        let result = evm.transact().unwrap();
        assert!(result.result.is_success());
    }
    let result = evm.transact().unwrap();
    assert!(result.result.is_success());

    println!("\t\t{}", result.result.into_output().unwrap());
}
