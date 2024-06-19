use evm_mlir::{
    db::{Bytecode, Db},
    env::{Env, TransactTo},
    primitives::Address,
    program::Program,
    Evm,
};
use num_bigint::BigUint;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("No path provided").as_str();
    let bytecode = std::fs::read(path).expect("Could not read file");
    let program = Program::from_bytecode(&bytecode);

    if let Err(err) = program {
        eprintln!("{:#?}", err);
        return;
    }

    let mut env = Env::default();
    env.tx.gas_limit = 999_999;

    let (address, bytecode) = (
        Address::from_low_u64_be(40),
        Bytecode::from(program.unwrap().to_bytecode()),
    );
    env.tx.transact_to = TransactTo::Call(address);
    let db = Db::new().with_bytecode(address, bytecode);
    let mut evm = Evm::new(env, db);

    let result = evm.transact();

    assert!(&result.is_success());
    let number = BigUint::from_bytes_be(result.return_data().unwrap());
    println!("Execution result: {number}");
}
