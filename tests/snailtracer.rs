use evm_mlir::{program::Program, Env, Evm};

const SNAILTRACER_BYTECODE: &[u8] = include_bytes!("../programs/snailtracer.bytecode");

#[test]
fn snailtracer() {
    println!("Running snailtracer example!");
    let start = std::time::Instant::now();
    let program = Program::from_bytecode(SNAILTRACER_BYTECODE);

    // BenchmarkDB is dummy state that implements Database trait.
    // let mut evm = Evm::builder()
    //     .with_db(BenchmarkDB::new_bytecode(bytecode.clone()))
    //     .modify_tx_env(|tx| {
    //         // execution globals block hash/gas_limit/coinbase/timestamp..
    //         tx.caller = address!("1000000000000000000000000000000000000000");
    //         tx.transact_to = TransactTo::Call(address!("0000000000000000000000000000000000000000"));
    //         tx.data = bytes!("30627b7c");
    //     })
    //     .build();

    let mut env = Env::default();
    env.tx.calldata = vec![48, 98, 123, 124];
    env.tx.gas_limit = 999_999;

    let evm = Evm::new(env, program);

    let _ = evm.transact();
    println!("elapsed: {:?}", start.elapsed());
}
