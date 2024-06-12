use evm_mlir::{program::Program, Env, Evm};
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
    env.tx.gas_limit = 1000;

    let evm = Evm::new(env, program.unwrap());

    let result = evm.transact();

    assert!(&result.is_success());
    let number = BigUint::from_bytes_be(result.return_data().unwrap());
    println!("Execution result: {number}");
}
