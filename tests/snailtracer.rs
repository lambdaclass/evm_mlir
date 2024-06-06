use evm_mlir::{program::Program, Env, Evm};

const SNAILTRACER_BYTECODE: &[u8] = include_bytes!("../programs/snailtracer.bytecode");

#[test]
fn snailtracer() {
    println!("Running snailtracer example!");
    let start = std::time::Instant::now();
    let program = Program::from_bytecode(SNAILTRACER_BYTECODE);

    let mut env = Env::default();
    env.tx.calldata = vec![48, 98, 123, 124];
    env.tx.gas_limit = 999_999;

    let evm = Evm::new(env, program);

    let _ = evm.transact();
    println!("elapsed: {:?}", start.elapsed());
}
