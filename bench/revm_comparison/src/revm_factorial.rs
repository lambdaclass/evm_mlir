use revm::{
    db::BenchmarkDB,
    primitives::{address, bytes, Bytecode, Bytes, TransactTo},
    Evm,
};

fn main() {
    const PROGRAM: Bytes = bytes!("7f0000000000000000000000000000000000000000000000000000000000000080600260025b8215603b57906001018091029160019003916025565b9150505f5260205ff3");
    const RUNS: usize = 100000;

    let raw = Bytecode::new_raw(PROGRAM.into());
    let mut evm = Evm::builder()
        .with_db(BenchmarkDB::new_bytecode(raw))
        .modify_tx_env(|tx| {
            tx.caller = address!("1000000000000000000000000000000000000000");
            tx.transact_to = TransactTo::Call(address!("0000000000000000000000000000000000000000"));
            tx.data = bytes!("");
        })
        .build();

    for _ in 0..RUNS {
        let result = evm.transact().unwrap();
        assert!(result.result.is_success());
    }
}
