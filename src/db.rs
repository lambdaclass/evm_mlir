#![allow(unused)]
use ethereum_types::{Address, U256};
use std::{collections::HashMap, fmt::Error};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bytecode(Vec<u8>);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DbAccount {
    pub nonce: u64,
    pub balance: U256,
    pub storage: HashMap<U256, U256>,
    pub bytecode_hash: B256,
}

type B256 = U256;

#[derive(Clone, Debug, Default)]
pub struct Db {
    accounts: HashMap<Address, DbAccount>,
    contracts: HashMap<B256, Bytecode>,
    block_hashes: HashMap<U256, B256>,
}

impl Db {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_storage(&mut self, address: Address, key: U256, value: U256) {
        let account = self.accounts.entry(address).or_default();
        account.storage.insert(key, value);
    }

    pub fn read_storage(&self, address: Address, key: U256) -> U256 {
        self.accounts
            .get(&address)
            .and_then(|account| account.storage.get(&key))
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Default, Clone, PartialEq, Debug)]
pub struct AccountInfo {
    /// Account balance.
    pub balance: U256,
    /// Account nonce.
    pub nonce: u64,
    /// code hash,
    pub code_hash: B256,
    /// code: if None, `code_by_hash` will be used to fetch it if code needs to be loaded from
    /// inside of `revm`.
    pub code: Option<Bytecode>,
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
        // Returns Ok(None) if no account with that address
        Ok(self.accounts.get(&address).map(|db_account| AccountInfo {
            balance: db_account.balance,
            nonce: db_account.nonce,
            code_hash: db_account.bytecode_hash,
            code: None,
        }))
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        // Returns Error if no contract with that address
        self.contracts.get(&code_hash).cloned().ok_or(DatabaseError)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        // Returns Ok(0) if no value with that address
        Ok(self.read_storage(address, index))
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        // Returns Error if no block with that number
        Ok(self
            .block_hashes
            .get(&number)
            .cloned()
            .ok_or(DatabaseError)?)
    }
}

#[cfg(test)]
mod tests {
    use melior::ir::block;

    use super::*;

    #[test]
    fn db_returns_basic_account_info() {
        let mut accounts = HashMap::new();
        let address = Address::default();
        let expected_account_info = AccountInfo::default();
        let db_account = DbAccount::default();
        accounts.insert(address, db_account);

        let mut db = Db {
            accounts,
            contracts: HashMap::new(),
            block_hashes: HashMap::new(),
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
        let mut db_account = DbAccount::default();
        db_account.storage.insert(index, expected_storage);
        accounts.insert(address, db_account);
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
