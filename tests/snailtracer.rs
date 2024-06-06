use evm_mlir::{env::Address, program::Program, Env, Evm};

const SNAILTRACER_BYTECODE: &[u8] = include_bytes!("../programs/snailtracer.bytecode");

#[test]
#[ignore]
// TODO: this test requires SSTORE, SLOAD, and CALLDATA related opcodes
fn snailtracer() {
    let program = Program::from_bytecode(SNAILTRACER_BYTECODE);

    let mut env = Env::default();
    env.tx.calldata = vec![48, 98, 123, 124];
    env.tx.gas_limit = 999_999;
    env.tx.from = Address([0; 20]);
    env.tx.from.0[0] = 16;
    env.tx.to = Address([0; 20]);

    let evm = Evm::new(env, program);

    let _ = evm.transact();
}
