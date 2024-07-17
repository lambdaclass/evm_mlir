use crate::{
    db::{AccountInfo, Bytecode, Db, DbAccount},
    primitives::{Address, B256, U256},
    state::{Account, AccountStatus, EvmStorageSlot},
};
use sha3::{Digest, Keccak256};
use std::collections::{hash_map::Entry, HashMap};
use thiserror::Error;

// NOTE: We could store the bytecode inside this `JournalAccount` instead of
// having a separate HashMap for it.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct JournalAccount {
    pub nonce: u64,
    pub balance: U256,
    pub storage: HashMap<U256, EvmStorageSlot>,
    pub bytecode_hash: B256,
    pub status: AccountStatus,
}

impl From<DbAccount> for JournalAccount {
    fn from(acc: DbAccount) -> JournalAccount {
        let DbAccount {
            nonce,
            storage,
            balance,
            bytecode_hash,
            status,
        } = acc;

        let storage = storage
            .iter()
            .map(|(key, &value)| (key.clone(), EvmStorageSlot::from(value)))
            .collect();

        JournalAccount {
            nonce,
            balance,
            storage,
            bytecode_hash,
            status,
        }
    }
}

impl From<&JournalAccount> for AccountInfo {
    fn from(acc: &JournalAccount) -> Self {
        Self {
            balance: acc.balance,
            nonce: acc.nonce,
            code_hash: acc.bytecode_hash,
            code: None,
        }
    }
}

type AccountState = HashMap<Address, JournalAccount>;
type ContractState = HashMap<B256, Bytecode>;

#[allow(dead_code)] //TODO: Delete this
#[derive(Default, Debug)]
pub struct Journal<'a> {
    accounts: AccountState,
    contracts: ContractState,
    block_hashes: HashMap<U256, B256>,
    db: Option<&'a mut Db>,
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
impl<'a> Journal<'a> {
    pub fn new(db: &'a mut Db) -> Self {
        Self {
            db: Some(db),
            ..Default::default()
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

        self.accounts.insert(address, account.into());
    }

    pub fn new_contract(&mut self, address: Address, bytecode: Bytecode, balance: U256) {
        let mut hasher = Keccak256::new();
        hasher.update(&bytecode);
        let hash = B256::from_slice(&hasher.finalize());
        let account = JournalAccount {
            bytecode_hash: hash,
            balance,
            nonce: 1,
            status: AccountStatus::Created,
            ..Default::default()
        };

        self.accounts.insert(address, account);
        self.contracts.insert(hash, bytecode);
    }

    // TODO: We should add some checks here
    pub fn set_balance(&mut self, address: &Address, balance: U256) {
        if let Some(acc) = self._get_account_mut(address) {
            acc.balance = balance;
            acc.status = AccountStatus::Touched;
        }
    }

    pub fn set_nonce(&mut self, address: &Address, nonce: u64) {
        if let Some(acc) = self._get_account_mut(address) {
            acc.nonce = nonce;
            acc.status = AccountStatus::Touched;
        }
    }

    pub fn set_status(&mut self, address: &Address, status: AccountStatus) {
        if let Some(acc) = self._get_account_mut(address) {
            acc.status = status;
        }
    }

    pub fn get_account(&mut self, address: &Address) -> Option<AccountInfo> {
        self._get_account(address).map(AccountInfo::from)
    }

    pub fn code_by_address(&mut self, address: &Address) -> Bytecode {
        self._get_account(address)
            .cloned()
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
        let _ = self._get_account(address);
    }

    pub fn prefetch_account_keys(&mut self, address: &Address, _keys: &Vec<U256>) {
        // NOTE: This prefetch implies a prefetch to the account too
        // Aren't all keys already in warm state if the account is in warm state?
        self.prefetch_account(address);
    }

    pub fn key_is_cold(&self, address: &Address, key: &U256) -> bool {
        self.accounts
            .get(address)
            .map(|acc| acc.storage.get(key))
            .flatten()
            .map(|slot| slot.is_cold)
            .unwrap_or(true)
    }

    /* STORAGE HANDLING */

    pub fn read_storage(&mut self, address: &Address, key: &U256) -> Option<EvmStorageSlot> {
        self._get_account(address)
            .and_then(|acc| acc.storage.get(key).cloned())
    }

    pub fn write_storage(&mut self, address: &Address, key: U256, value: U256) {
        // TODO: We might do an implace modification here
        let Some(mut acc) = self._get_account(address).cloned() else {
            //TODO: This might return error on this case
            return;
        };

        let slot = match acc.storage.get(&key) {
            Some(slot) => EvmStorageSlot {
                original_value: slot.original_value,
                present_value: value,
                is_cold: false,
            },
            None => EvmStorageSlot {
                original_value: value,
                present_value: value,
                is_cold: false,
            },
        };

        acc.storage.insert(key, slot);
        acc.status = AccountStatus::Touched;
        self.accounts.insert(*address, acc.clone());
    }

    /* BLOCK HASH */

    pub fn get_block_hash(&mut self, number: &U256) -> B256 {
        self.block_hashes
            .get(&number)
            .cloned()
            .unwrap_or(B256::zero())
    }

    /* OTHER METHODS */

    //NOTE: Here we are loosing the bytecode
    //Idk why bytecode is separated from the Account
    pub fn into_state(&self) -> HashMap<Address, Account> {
        self.accounts
            .iter()
            .map(|(address, acc)| {
                (
                    *address,
                    Account {
                        info: AccountInfo::from(acc),
                        storage: acc.storage.clone(),
                        status: acc.status,
                    },
                )
            })
            .collect()
    }

    pub fn eject_base(&mut self) -> Self {
        Self {
            accounts: self.accounts.clone(),
            contracts: self.contracts.clone(),
            block_hashes: self.block_hashes.clone(),
            db: self.db.take(),
        }
    }

    pub fn extend_from_successful(&mut self, other: Journal<'a>) {
        self.accounts = other.accounts;
        self.contracts = other.contracts;
        self.block_hashes = other.block_hashes;
        self.db = other.db;
    }

    pub fn extend_from_reverted(&mut self, other: Journal<'a>) {
        //TODO: Copy new fetched accounts / contracts so we preserve warm/cold state
        //Caution with modified state, should only copy if state is the same as Db
        //-> Check AccountStatus flags
        self.extend_from_successful(other);
    }

    /* PRIVATE AUXILIARY METHODS */

    fn _get_account(&mut self, address: &Address) -> Option<&JournalAccount> {
        let Some(db) = &mut self.db else {
            return None;
        };

        // NOTE: This may be simplified replacing the second match with a map or and_then
        // but I'm having trouble with the borrow checker
        let maybe_acc: Option<&JournalAccount> = match self.accounts.entry(*address) {
            Entry::Occupied(e) => Some(e.into_mut()),
            Entry::Vacant(e) => match db.get_account(address).cloned() {
                Some(acc) => {
                    let mut acc = JournalAccount::from(acc);
                    acc.status = AccountStatus::Loaded;
                    Some(e.insert(acc))
                }
                None => None,
            },
        };

        maybe_acc
    }

    fn _get_account_mut(&mut self, address: &Address) -> Option<&mut JournalAccount> {
        let Some(db) = &mut self.db else {
            return None;
        };

        // NOTE: This may be simplified replacing the second match with a map or and_then
        // but I'm having trouble with the borrow checker
        let maybe_acc = match self.accounts.entry(*address) {
            Entry::Occupied(e) => Some(e.into_mut()),
            Entry::Vacant(e) => match db.get_account(address).cloned() {
                Some(acc) => {
                    let mut acc = JournalAccount::from(acc);
                    acc.status = AccountStatus::Loaded;
                    Some(e.insert(acc))
                }
                None => None,
            },
        };

        maybe_acc
    }
}
