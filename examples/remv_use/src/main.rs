use revm::{
    db::BenchmarkDB,
    primitives::{address, bytes, Bytecode, Bytes, TransactTo},
    Evm,
};

const PROGRAM: Bytes = bytes!("7f00000000000000000000000000000000000000000000000000000000000003e75f60015b82156039578181019150909160019003916024565b9150505f5260205ff3");

fn main() {
    const RUNS: usize = 10000;

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
