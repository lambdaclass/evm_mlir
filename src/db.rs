#![allow(unused)]
use crate::{
    constants::EMPTY_CODE_HASH_STR,
    primitives::{Address, Bytes, B256, U256},
    state::{Account, AccountStatus, EvmStorageSlot},
};
use core::fmt;
use sha3::{Digest, Keccak256};
use std::str::FromStr;
use std::{collections::HashMap, convert::Infallible, fmt::Error, ops::Add};
use thiserror::Error;
pub type Bytecode = Bytes;

#[derive(Clone, Default, Debug, PartialEq)]
pub struct DbAccount {
    pub nonce: u64,
    pub balance: U256,
    pub storage: HashMap<U256, U256>,
    pub bytecode_hash: B256,
    pub status: AccountStatus,
}

impl DbAccount {
    fn newly_created() -> Self {
        DbAccount {
            nonce: 1,
            balance: U256::zero(),
            storage: HashMap::new(),
            bytecode_hash: B256::from_str(EMPTY_CODE_HASH_STR).unwrap(),
            status: AccountStatus::Created,
        }
    }
}

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

    pub fn insert_block_hash(&mut self, number: U256, hash: B256) {
        self.block_hashes.insert(number, hash);
    }

    pub fn set_account(
        &mut self,
        address: Address,
        nonce: u64,
        balance: U256,
        storage: HashMap<U256, U256>,
    ) {
        let a = self.accounts.entry(address).or_default();
        a.nonce = nonce;
        a.balance = balance;
        a.storage = storage;
    }

    pub fn set_balance(&mut self, address: Address, balance: U256) {
        let account = self.accounts.entry(address).or_default();
        account.balance = balance;
    }

    pub fn get_balance(&mut self, address: Address) -> Option<U256> {
        self.accounts.get(&address).map(|acc| acc.balance)
    }

    pub fn address_is_created(&self, address: Address) -> bool {
        self.accounts
            .get(&address)
            .map(|acc| acc.status.contains(AccountStatus::Created))
            .unwrap_or(false)
    }

    pub fn set_status(&mut self, address: Address, status: AccountStatus) {
        let a = self.accounts.entry(address).or_default();
        a.status = status;
    }

    pub fn with_contract(mut self, address: Address, bytecode: Bytecode) -> Self {
        self.insert_contract(address, bytecode, U256::zero());
        self
    }

    pub fn insert_contract(&mut self, address: Address, bytecode: Bytecode, balance: U256) {
        let mut hasher = Keccak256::new();
        hasher.update(&bytecode);
        let hash = B256::from_slice(&hasher.finalize());
        let account = DbAccount {
            bytecode_hash: hash,
            balance,
            ..DbAccount::newly_created()
        };

        self.accounts.insert(address, account);
        self.contracts.insert(hash, bytecode);
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
            .unwrap_or(U256::zero())
    }

    pub fn into_state(self) -> HashMap<Address, Account> {
        self.accounts
            .iter()
            .map(|(address, db_account)| {
                (
                    *address,
                    Account {
                        info: AccountInfo::from(db_account.clone()),
                        storage: db_account
                            .storage
                            .iter()
                            .map(|(k, v)| (*k, EvmStorageSlot::from(*v)))
                            .collect(),
                        status: db_account.status,
                    },
                )
            })
            .collect()
    }
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
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

impl AccountInfo {
    pub fn is_empty(&self) -> bool {
        self.balance.is_zero()
            && self.nonce == 0
            && self.code_hash == B256::from_str(EMPTY_CODE_HASH_STR).unwrap()
    }
}

impl From<DbAccount> for AccountInfo {
    fn from(db_account: DbAccount) -> Self {
        Self {
            balance: db_account.balance,
            nonce: db_account.nonce,
            code_hash: db_account.bytecode_hash,
            code: None,
        }
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

    /// Get account code by its address.
    fn code_by_address(&mut self, address: Address) -> Result<Bytecode, Self::Error> {
        let code = self
            .basic(address)?
            .and_then(|acc| acc.code.or_else(|| self.code_by_hash(acc.code_hash).ok()))
            .unwrap_or_default();
        Ok(code)
    }
}

#[derive(Error, Debug, Clone, Hash, PartialEq, Eq)]
#[error("error on database access")]
pub struct DatabaseError;

impl Database for Db {
    type Error = Infallible;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        // Returns Ok(None) if no account with that address
        // TODO: this can be done more efficently if the storage is not cloned
        Ok(self.accounts.get(&address).cloned().map(AccountInfo::from))
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        Ok(self.contracts.get(&code_hash).cloned().unwrap_or_default())
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        // Returns Ok(0) if no value with that address
        Ok(self.read_storage(address, index))
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        Ok(self.block_hashes.get(&number).cloned().unwrap_or_default())
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
        let expected_hash = B256::from_low_u64_be(2);
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
