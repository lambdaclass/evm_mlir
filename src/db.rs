use std::collections::HashMap;

use ethereum_types::{H256, U256};

use crate::env::Address;

#[derive(Clone, Debug, Default)]
pub struct Bytecode(Vec<u8>);

#[derive(Clone, Debug, Default)]
struct AccountInfo {
    nonce: u64,
    balance: U256,
    storage: HashMap<U256, U256>,
    bytecode: Bytecode,
}

#[derive(Clone, Debug, Default)]
pub struct Db {
    accounts: HashMap<Address, AccountInfo>,
}
