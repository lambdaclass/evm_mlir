#![allow(unused)]
use ethereum_types::{Address, U256};
use std::{collections::HashMap, fmt::Error};

#[derive(Clone, Debug, Default)]
pub struct Bytecode(Vec<u8>);

#[derive(Clone, Debug, Default)]
pub struct AccountInfo {
    nonce: u64,
    balance: U256,
    storage: HashMap<U256, U256>,
    bytecode: Bytecode,
}

#[derive(Clone, Debug, Default)]
pub struct Db {
    accounts: HashMap<Address, AccountInfo>,
}

type B256 = U256;

pub trait Database {
    /// The database error type.
    type Error;

    /// Get basic account information.
    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error>;

    /// Get account code by its hash.
    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error>;

    /// Get storage value of address at index.
    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error>;

    /// Get block hash by block number.
    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error>;
}

impl Database for Db {
    type Error = Error; // TODO: implement error

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        unimplemented!()
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        unimplemented!()
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        //iterate the hashmap self.accounts and return the storage value of the address at index
        for (iter_address, account_info) in self.accounts.iter() {
            if *iter_address == address {
                match account_info.storage.get(&index) {
                    Some(value) => return Ok(*value),
                    None => return Ok(U256::default()),
                }
            }
        }
        Ok(U256::default())
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        unimplemented!()
    }
}
