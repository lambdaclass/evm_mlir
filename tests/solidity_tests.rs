use std::io::Read;

use bytes::Bytes;
use ethereum_types::Address;
use evm_mlir::{db::Db, env::TransactTo, Env, Evm};

fn read_compiled_file(file_path: &str) -> Result<Bytes, std::io::Error> {
    let mut file = std::fs::File::open(file_path)?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)?;
    Ok(Bytes::from(hex::decode(buffer).unwrap()))
}

#[test]
fn factorial_contract() {
    let address = Address::from_low_u64_be(3000);
    let bytes = read_compiled_file("./compiled/Factorial.bin").unwrap();
    let db = Db::new().with_contract(address, bytes);
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.transact_to = TransactTo::Call(address);
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap();
    assert!(result.result.is_success());
    let state = result.state.get(&address).unwrap();
    assert_eq!(
        state
            .storage
            .get(&ethereum_types::U256::zero())
            .unwrap()
            .present_value,
        ethereum_types::U256::from(3628800) // 10!
    )
}

#[test]
fn fibonacci_contract() {
    let address = Address::from_low_u64_be(3000);
    let bytes = read_compiled_file("./compiled/Fibonacci.bin").unwrap();

    let db = Db::new().with_contract(address, bytes);
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.transact_to = TransactTo::Call(address);
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap();
    assert!(result.result.is_success());
    let state = result.state.get(&address).unwrap();
    assert_eq!(
        state
            .storage
            .get(&ethereum_types::U256::zero())
            .unwrap()
            .present_value,
        ethereum_types::U256::from(55) // fibonacci(10)
    )
}

#[test]
fn recursive_fibonacci_contract() {
    let address = Address::from_low_u64_be(3000);
    let bytes = read_compiled_file("./compiled/RecursiveFibonacci.bin").unwrap();

    let db = Db::new().with_contract(address, bytes);
    let mut env = Env::default();
    env.tx.gas_limit = 999_999;
    env.tx.transact_to = TransactTo::Call(address);
    let mut evm = Evm::new(env, db);
    let result = evm.transact().unwrap();
    assert!(result.result.is_success());
    let state = result.state.get(&address).unwrap();
    assert_eq!(
        state
            .storage
            .get(&ethereum_types::U256::zero())
            .unwrap()
            .present_value,
        ethereum_types::U256::from(55) // fibonacci(10)
    )
}
