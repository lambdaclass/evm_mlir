use evm_mlir::{
    env::TransactTo,
    primitives::{Address, Bytes},
    program::Program,
    Env, Evm,
};

const SNAILTRACER_BYTECODE: &[u8] = include_bytes!("../programs/snailtracer.bytecode");

#[test]
#[ignore]
// TODO: this test requires SSTORE, SLOAD, and CALLDATA related opcodes
fn snailtracer() {
    let program = Program::from_bytecode(SNAILTRACER_BYTECODE);

    let mut env = Env::default();
    env.tx.data = Bytes::from(vec![48, 98, 123, 124]);
    env.tx.gas_limit = 999_999;
    let mut caller_address = vec![0x0; 160];
    caller_address[0] = 16;
    env.tx.caller = Address::from_slice(&caller_address);
    env.tx.transact_to = TransactTo::Call(Address::zero());

    let evm = Evm::new(env, program.expect("Error parsing opcodes"));

    let _ = evm.transact();
}
