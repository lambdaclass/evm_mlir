#![allow(unused)]
use ethereum_types::{Address, U256};
use std::{collections::HashMap, fmt::Error};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bytecode(pub Vec<u8>);

impl Bytecode {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountInfo {
    nonce: u64,
    balance: U256,
    storage: HashMap<U256, U256>,
    pub bytecode: Bytecode,
}

type B256 = U256;

#[derive(Clone, Debug, Default)]
pub struct Db {
    accounts: HashMap<Address, AccountInfo>,
    pub contracts: HashMap<B256, Bytecode>,
    block_hashes: HashMap<U256, B256>,
}

impl Db {
    pub fn with_bytecode(address: Address, bytecode: Bytecode) -> Self {
        let mut db = Db::default();
        let account_info = AccountInfo {
            bytecode,
            ..Default::default()
        };
        db.accounts.insert(address, account_info);
        db
    }

    pub fn write_storage(&mut self, address: Address, key: U256, value: U256) {
        let account_info = self.accounts.entry(address).or_default();
        account_info.storage.insert(key, value);
    }

    pub fn read_storage(&self, address: Address, key: U256) -> U256 {
        self.accounts
            .get(&address)
            .and_then(|account_info| account_info.storage.get(&key))
            .cloned()
            .unwrap_or_default()
    }
}

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

#[derive(Debug, Clone)]
pub struct DatabaseError;

impl Database for Db {
    type Error = DatabaseError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        Ok(self.accounts.get(&address).cloned())
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.contracts.get(&code_hash).cloned().ok_or(DatabaseError)
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
        match self.block_hashes.entry(number) {
            std::collections::hash_map::Entry::Occupied(entry) => Ok(*entry.get()),
            std::collections::hash_map::Entry::Vacant(entry) => Ok(B256::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use melior::ir::block;

    use super::*;

    #[test]
    fn db_returns_basic_account_info() {
        let mut accounts = HashMap::new();
        let block_hashes = HashMap::new();
        let address = Address::default();
        let expected_account_info = AccountInfo::default();
        accounts.insert(address, expected_account_info.clone());
        let mut db = Db {
            accounts,
            contracts: HashMap::new(),
            block_hashes,
        };

        let account_info = db.basic(address).unwrap().unwrap();

        assert_eq!(account_info, expected_account_info);
    }

    #[test]
    fn db_returns_code_by_hash() {
        let mut contracts = HashMap::new();
        let block_hashes = HashMap::new();
        let hash = B256::default();
        let expected_bytecode = Bytecode::default();
        contracts.insert(hash, expected_bytecode.clone());
        let mut db = Db {
            accounts: HashMap::new(),
            contracts,
            block_hashes,
        };

        let bytecode = db.code_by_hash(hash).unwrap();

        assert_eq!(bytecode, expected_bytecode);
    }

    #[test]
    fn db_returns_storage() {
        let mut accounts = HashMap::new();
        let block_hashes = HashMap::new();
        let address = Address::default();
        let index = U256::from(1);
        let expected_storage = U256::from(2);
        let mut account_info = AccountInfo::default();
        account_info.storage.insert(index, expected_storage);
        accounts.insert(address, account_info);
        let mut db = Db {
            accounts,
            contracts: HashMap::new(),
            block_hashes,
        };

        let storage = db.storage(address, index).unwrap();

        assert_eq!(storage, expected_storage);
    }

    #[test]
    fn db_returns_block_hash() {
        let accounts = HashMap::new();
        let mut block_hashes = HashMap::new();
        let number = U256::from(1);
        let expected_hash = B256::from(2);
        block_hashes.insert(number, expected_hash);
        let mut db = Db {
            accounts,
            contracts: HashMap::new(),
            block_hashes,
        };

        let hash = db.block_hash(number).unwrap();

        assert_eq!(hash, expected_hash);
    }
}
