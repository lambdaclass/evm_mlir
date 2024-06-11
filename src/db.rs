use std::collections::HashMap;

use ethereum_types::U256;

pub struct Bytecode(Vec<u8>);

#[derive(Clone, Debug, Default)]
pub struct Db {
    pub nonce: u64,
    pub balance: U256,
    pub storage: HashMap<U256, U256>,
    pub bytecode: Bytecode,
}