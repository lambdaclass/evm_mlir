use crate::{
    db::{AccountInfo, Bytecode, Db, DbAccount},
    primitives::{Address, B256, U256},
    state::{Account, AccountStatus, EvmStorageSlot},
};
use sha3::{Digest, Keccak256};
use std::collections::HashMap;
use thiserror::Error;

type AccountState = HashMap<Address, DbAccount>;
type ContractState = HashMap<B256, Bytecode>;

#[allow(dead_code)] //TODO: Delete this
#[derive(Default)]
pub struct Journal {
    accounts: AccountState,
    contracts: ContractState,
    db: Db,
}
#[derive(Error, Debug)]
#[error("Journal Error")]
pub struct JournalError;

// TODO: Handle unwraps and panics
// TODO: Improve overall performance
//  -> Performance is not the focus currently
//  -> Many copies, clones and Db fetches that may be reduced
//  -> For the moment we seek for something that works.
//  -> We can optimize in the future.
#[allow(dead_code)] //TODO: Delete this
impl Journal {
    pub fn new(db: Db) -> Self {
        Self {
            db,
            ..Default::default()
        }
    }

    pub fn from_state(accounts: AccountState, contracts: ContractState, db: Db) -> Self {
        Self {
            accounts,
            contracts,
            db,
        }
    }

    /* ACCOUNT HANDLING */

    //TODO: Check if we really need to pass an init storage
    pub fn new_account(&mut self, address: Address, balance: U256, storage: HashMap<U256, U256>) {
        // TODO: Check if account already exists and return error or panic
        let account = DbAccount {
            balance,
            storage,
            status: AccountStatus::Created,
            ..Default::default()
        };
        self.accounts.insert(address, account);
    }

    pub fn new_contract(&mut self, address: Address, bytecode: Bytecode, balance: U256) {
        let mut hasher = Keccak256::new();
        hasher.update(&bytecode);
        let hash = B256::from_slice(&hasher.finalize());
        let account = DbAccount {
            bytecode_hash: hash,
            balance,
            nonce: 1,
            status: AccountStatus::Created,
            ..Default::default()
        };

        self.accounts.insert(address, account);
        self.contracts.insert(hash, bytecode);
    }

    pub fn get_account(&mut self, address: &Address) -> Option<AccountInfo> {
        self._get_account(address).map(AccountInfo::from)
    }

    fn code_by_address(&mut self, address: &Address) -> Bytecode {
        self._get_account(address)
            .and_then(|acc| self.contracts.get(&acc.bytecode_hash).cloned())
            .unwrap_or_default()
    }

    /* WARM COLD HANDLING */

    pub fn account_is_warm(&self, address: &Address) -> bool {
        self.accounts
            .get(address)
            .map(|acc| !matches!(acc.status, AccountStatus::Cold))
            .unwrap_or(false)
    }

    pub fn prefetch_account(&mut self, address: &Address) {
        let maybe_acc = self._get_account(address);
        if let Some(mut acc) = maybe_acc {
            if matches!(acc.status, AccountStatus::Cold) {
                acc.status = AccountStatus::Loaded;
            }
            self.accounts.insert(*address, acc);
        }
    }

    pub fn prefetch_account_keys(&mut self, address: &Address, _keys: &Vec<U256>) {
        // NOTE: This prefetch implies a prefetch to the account too
        // It may have more sense in the future, since for the moment an account prefetch
        // is identical to a key prefetch, because all keys are stored in `DbAccount` struct
        // This depends on the database API
        self.prefetch_account(address);
    }

    pub fn key_is_warm(&self, address: &Address, key: &U256) -> bool {
        self.accounts
            .get(address)
            .map(|acc| acc.storage.get(key).is_some())
            .unwrap_or(false)
    }

    /* STORAGE HANDLING */

    pub fn read_storage(&mut self, address: &Address, key: &U256) -> Option<U256> {
        self._get_account(address)
            .and_then(|acc| acc.storage.get(key).copied())
    }

    pub fn write_storage(&mut self, address: &Address, key: U256, value: U256) {
        let Some(mut acc) = self._get_account(address) else {
            return;
        };
        acc.storage.insert(key, value);
        self.accounts.insert(*address, acc);
    }

    /* OTHER METHODS */

    //NOTE: Here we are loosing the bytecode
    //Idk why bytecode is separated from the Account
    pub fn into_state(&self) -> HashMap<Address, Account> {
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

    pub fn eject(self) -> (AccountState, ContractState, Db) {
        (self.accounts, self.contracts, self.db)
    }

    pub fn extend_from_successful(&mut self, accounts: AccountState, contracts: ContractState) {
        self.accounts = accounts;
        self.contracts = contracts;
    }

    pub fn extend_from_reverted(&mut self, _accounts: AccountState, _contracts: ContractState) {
        //TODO: Copy new fetched accounts / contracts so we preserve warm/cold state
        //Caution with modified state, should only copy if state is the same as Db
        //-> Check AccountStatus flags
        panic!("not implemented");
    }

    /* PRIVATE AUXILIARY METHODS */

    fn _get_account(&mut self, address: &Address) -> Option<DbAccount> {
        let (acc, from_db) = match self.accounts.get(address).cloned() {
            Some(acc) => (Some(acc), false),
            None => (self.db.get_account(address).cloned(), true),
        };

        if acc.is_some() && from_db {
            // We store the fetched account into state
            let mut acc = acc.clone().unwrap();
            acc.status = AccountStatus::Loaded;
            let _ = self.accounts.insert(address.to_owned(), acc);
        }

        acc
    }
}
