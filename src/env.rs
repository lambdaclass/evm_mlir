use std::cmp::Ordering;

use crate::{
    constants::{
        gas_cost::{
            init_code_cost, MAX_CODE_SIZE, TX_BASE_COST, TX_CREATE_COST, TX_DATA_COST_PER_NON_ZERO,
            TX_DATA_COST_PER_ZERO,
        },
        MAX_BLOB_NUMBER_PER_BLOCK, VERSIONED_HASH_VERSION_KZG,
    },
    db::AccountInfo,
    primitives::{Address, Bytes, B256, U256},
    result::InvalidTransaction,
    utils::{access_list_cost, calc_blob_gasprice},
};

pub type AccessList = Vec<(Address, Vec<U256>)>;

//This Env struct contains configuration information about the EVM, the block containing the transaction, and the transaction itself.
//Structs inspired by the REVM primitives
//-> https://github.com/bluealloy/revm/blob/main/crates/primitives/src/env.rs
// moved to: https://github.com/bluealloy/revm/blob/be1d324298b6a1e20f8b17aff34f95206304117b/crates/wiring/src/default.rs#L31
#[derive(Clone, Debug, Default)]
pub struct Env {
    /// Configuration of the EVM itself.
    pub cfg: CfgEnv,
    /// Configuration of the block the transaction is in.
    pub block: BlockEnv,
    /// Configuration of the transaction that is being executed.
    pub tx: TxEnv,
}

impl Env {
    pub fn consume_intrinsic_cost(&mut self) -> Result<u64, InvalidTransaction> {
        let intrinsic_cost = self.calculate_intrinsic_cost();
        if self.tx.gas_limit >= intrinsic_cost {
            self.tx.gas_limit -= intrinsic_cost;
            Ok(intrinsic_cost)
        } else {
            Err(InvalidTransaction::CallGasCostMoreThanGasLimit)
        }
    }

    /// Checks if the transaction is valid.
    ///
    /// See the [execution spec] for reference.
    ///
    /// [execution spec]: https://github.com/ethereum/execution-specs/blob/c854868f4abf2ab0c3e8790d4c40607e0d251147/src/ethereum/cancun/fork.py#L332
    pub fn validate_transaction(&self, account: &AccountInfo) -> Result<(), InvalidTransaction> {
        // if initial tx gas cost (intrinsic cost) is greater that tx limit
        // https://github.com/ethereum/execution-specs/blob/c854868f4abf2ab0c3e8790d4c40607e0d251147/src/ethereum/cancun/fork.py#L372
        // https://github.com/bluealloy/revm/blob/66adad00d8b89f1ab4057297b95b975564575fd4/crates/interpreter/src/gas/calc.rs#L362
        let intrinsic_cost = self.calculate_intrinsic_cost();

        if intrinsic_cost > self.tx.gas_limit {
            return Err(InvalidTransaction::CallGasCostMoreThanGasLimit);
        }

        // if nonce is None, nonce check skipped
        // https://github.com/ethereum/execution-specs/blob/c854868f4abf2ab0c3e8790d4c40607e0d251147/src/ethereum/cancun/fork.py#L419
        if let Some(tx) = self.tx.nonce {
            let state = account.nonce;

            match tx.cmp(&state) {
                Ordering::Greater => return Err(InvalidTransaction::NonceTooHigh { tx, state }),
                Ordering::Less => return Err(InvalidTransaction::NonceTooLow { tx, state }),
                Ordering::Equal => {}
            }
        }

        // if it's a create tx, check max code size
        // https://github.com/ethereum/execution-specs/blob/c854868f4abf2ab0c3e8790d4c40607e0d251147/src/ethereum/cancun/fork.py#L376
        let is_create = matches!(self.tx.transact_to, TransactTo::Create);

        if is_create && self.tx.data.len() > 2 * MAX_CODE_SIZE {
            return Err(InvalidTransaction::CreateInitCodeSizeLimit);
        }

        // if the tx gas limit is greater than the available gas in the block
        // https://github.com/ethereum/execution-specs/blob/c854868f4abf2ab0c3e8790d4c40607e0d251147/src/ethereum/cancun/fork.py#L379
        if U256::from(self.tx.gas_limit) > self.block.gas_limit {
            return Err(InvalidTransaction::CallerGasLimitMoreThanBlock);
        }

        // transactions from callers with deployed code should be rejected
        // this is formalized on EIP-3607: https://eips.ethereum.org/EIPS/eip-3607
        // https://github.com/ethereum/execution-specs/blob/c854868f4abf2ab0c3e8790d4c40607e0d251147/src/ethereum/cancun/fork.py#L423
        if account.has_code() {
            return Err(InvalidTransaction::RejectCallerWithCode);
        }

        // if it's a fee market tx (eip-1559)
        if let Some(max_priority_fee_per_gas) = self.tx.gas_priority_fee {
            // the max tip fee i'm willing to pay can't exceed the
            // max total fee i'm willing to pay
            // https://github.com/ethereum/execution-specs/blob/c854868f4abf2ab0c3e8790d4c40607e0d251147/src/ethereum/cancun/fork.py#L386
            if self.tx.gas_price < max_priority_fee_per_gas {
                return Err(InvalidTransaction::PriorityFeeGreaterThanMaxFee);
            }
        }

        // the max fee i'm willing to pay for the tx can't be
        // less than the block's base fee
        // https://github.com/ethereum/execution-specs/blob/c854868f4abf2ab0c3e8790d4c40607e0d251147/src/ethereum/cancun/fork.py#L388
        if self.tx.gas_price < self.block.basefee {
            return Err(InvalidTransaction::GasPriceLessThanBasefee);
        }

        if let Some(max) = self.tx.max_fee_per_blob_gas {
            let price = self.block.blob_gasprice.unwrap();
            if U256::from(price) > max {
                return Err(InvalidTransaction::BlobGasPriceGreaterThanMax);
            }
            if self.tx.blob_hashes.is_empty() {
                return Err(InvalidTransaction::EmptyBlobs);
            }
            if is_create {
                return Err(InvalidTransaction::BlobCreateTransaction);
            }
            for blob in self.tx.blob_hashes.iter() {
                if blob[0] != VERSIONED_HASH_VERSION_KZG {
                    return Err(InvalidTransaction::BlobVersionNotSupported);
                }
            }

            let num_blobs = self.tx.blob_hashes.len();
            if num_blobs > MAX_BLOB_NUMBER_PER_BLOCK as usize {
                return Err(InvalidTransaction::TooManyBlobs {
                    have: num_blobs,
                    max: MAX_BLOB_NUMBER_PER_BLOCK as usize,
                });
            }
        }
        // TODO: check if more validations are needed
        Ok(())
    }

    /// Calculates the gas that is charged before execution is started.
    ///
    /// See the [revm implementation], or the [execution spec implementation] for reference.
    ///
    /// [execution spec]: https://github.com/ethereum/execution-specs/blob/c854868f4abf2ab0c3e8790d4c40607e0d251147/src/ethereum/cancun/fork.py#L812
    /// [revm implementation]: https://github.com/bluealloy/revm/blob/66adad00d8b89f1ab4057297b95b975564575fd4/crates/interpreter/src/gas/calc.rs#L362
    pub fn calculate_intrinsic_cost(&self) -> u64 {
        let data_cost = self.tx.data.iter().fold(0, |acc, byte| {
            acc + if *byte == 0 {
                TX_DATA_COST_PER_ZERO
            } else {
                TX_DATA_COST_PER_NON_ZERO
            }
        });

        let create_cost = match self.tx.transact_to {
            TransactTo::Call(_) => 0,
            TransactTo::Create => TX_CREATE_COST + init_code_cost(self.tx.data.len()),
        };

        let access_list_cost = access_list_cost(&self.tx.access_list);

        TX_BASE_COST + data_cost + create_cost + access_list_cost
    }
}

#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct CfgEnv {
    // Chain ID of the EVM, it will be compared to the transaction's Chain ID.
    // Chain ID is introduced EIP-155
    pub chain_id: u64,
    // Bytecode that is created with CREATE/CREATE2 is by default analysed and jumptable is created.
    // This is very beneficial for testing and speeds up execution of that bytecode if called multiple times.
    //
    // Default: Analyse
    //pub perf_analyse_created_bytecodes: AnalysisKind,
    // If some it will effects EIP-170: Contract code size limit. Useful to increase this because of tests.
    // By default it is 0x6000 (~25kb).
    //pub limit_contract_code_size: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct BlockEnv {
    /// The number of ancestor blocks of this block (block height).
    pub number: U256,
    /// Coinbase or miner or address that created and signed the block.
    ///
    /// This is the receiver address of all the gas spent in the block.
    pub coinbase: Address,
    /// The timestamp of the block in seconds since the UNIX epoch.
    pub timestamp: U256,
    // The gas limit of the block.
    pub gas_limit: U256,
    ///
    /// The base fee per gas, added in the London upgrade with [EIP-1559].
    ///
    /// [EIP-1559]: https://eips.ethereum.org/EIPS/eip-1559
    /// aka `base_fee_per_gas`
    pub basefee: U256,
    // The difficulty of the block.
    //
    // Unused after the Paris (AKA the merge) upgrade, and replaced by `prevrandao`.
    //pub difficulty: U256,
    // The output of the randomness beacon provided by the beacon chain.
    //
    // Replaces `difficulty` after the Paris (AKA the merge) upgrade with [EIP-4399].
    //
    // NOTE: `prevrandao` can be found in a block in place of `mix_hash`.
    //
    // [EIP-4399]: https://eips.ethereum.org/EIPS/eip-4399
    pub prevrandao: Option<B256>,
    // Excess blob gas and blob gasprice.
    // See also [`crate::calc_excess_blob_gas`]
    // and [`calc_blob_gasprice`].
    //
    // Incorporated as part of the Cancun upgrade via [EIP-4844].
    //
    // [EIP-4844]: https://eips.ethereum.org/EIPS/eip-4844
    pub excess_blob_gas: Option<u64>,
    pub blob_gasprice: Option<u128>,
}

impl BlockEnv {
    pub fn set_blob_base_fee(&mut self, excess_blob_gas: u64) {
        self.excess_blob_gas = Some(excess_blob_gas);
        self.blob_gasprice = Some(calc_blob_gasprice(excess_blob_gas));
    }
}

/// The transaction environment.
#[derive(Clone, Debug)]
pub struct TxEnv {
    /// Caller aka Author aka transaction signer.
    pub caller: Address,
    /// The gas limit of the transaction.
    pub gas_limit: u64,
    /// The gas price of the transaction.
    /// aka `max_fee_per_gas`
    pub gas_price: U256,
    /// The destination of the transaction.
    pub transact_to: TransactTo,
    /// The value sent to `transact_to`.
    pub value: U256,
    // The data of the transaction.
    pub data: Bytes,
    // The nonce of the transaction.
    pub nonce: Option<u64>,
    // Caution: If set to `None`, then nonce validation against the account's nonce is skipped: [InvalidTransaction::NonceTooHigh] and [InvalidTransaction::NonceTooLow]

    // The chain ID of the transaction. If set to `None`, no checks are performed.
    //
    // Incorporated as part of the Spurious Dragon upgrade via [EIP-155].
    //
    // [EIP-155]: https://eips.ethereum.org/EIPS/eip-155
    // pub chain_id: Option<u64>,

    // A list of addresses and storage keys that the transaction plans to access.
    //
    // Added in [EIP-2930].
    //
    // [EIP-2930]: https://eips.ethereum.org/EIPS/eip-2930
    pub access_list: AccessList,

    /// The priority fee per gas.
    ///
    /// Incorporated as part of the London upgrade via [EIP-1559].
    ///
    /// [EIP-1559]: https://eips.ethereum.org/EIPS/eip-1559
    /// aka `max_priority_fee_per_gas` or _miner tip_
    pub gas_priority_fee: Option<U256>,

    // The list of blob versioned hashes. Per EIP there should be at least
    // one blob present if [`Self::max_fee_per_blob_gas`] is `Some`.
    //
    // Incorporated as part of the Cancun upgrade via [EIP-4844].
    //
    // [EIP-4844]: https://eips.ethereum.org/EIPS/eip-4844
    pub blob_hashes: Vec<B256>,
    // The max fee per blob gas.
    //
    // Incorporated as part of the Cancun upgrade via [EIP-4844].
    //
    // [EIP-4844]: https://eips.ethereum.org/EIPS/eip-4844
    pub max_fee_per_blob_gas: Option<U256>,
}

impl Default for TxEnv {
    fn default() -> Self {
        Self {
            caller: Address::zero(),
            // TODO: we are using signed comparison for the gas counter
            gas_limit: i64::MAX as _,
            gas_price: U256::zero(),
            gas_priority_fee: None,
            transact_to: TransactTo::Call(Address::zero()),
            value: U256::zero(),
            data: Bytes::new(),
            // chain_id: None,
            nonce: None,
            access_list: Default::default(),
            blob_hashes: Vec::new(),
            max_fee_per_blob_gas: None,
        }
    }
}

/// Transaction destination.
#[derive(Clone, Debug)]
pub enum TransactTo {
    /// Simple call to an address.
    Call(Address),
    /// Contract creation.
    Create,
}

impl TxEnv {
    pub fn get_address(&self) -> Address {
        match self.transact_to {
            TransactTo::Call(addr) => addr,
            TransactTo::Create => self.caller,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ethereum_types::H160;

    use crate::db::{Bytecode, Db, DbAccount};

    use super::*;

    #[test]
    /// Tx invalid if tx nonce > caller nonce.
    fn validation_nonce_too_high() {
        let tx_env = TxEnv {
            nonce: Some(42),
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            ..Default::default()
        };

        let mut db = Db::default();
        db.set_account(Address::default(), 41, U256::MAX, HashMap::default());

        let tx_result =
            env.validate_transaction(&db.get_account(Address::default()).unwrap().clone().into());

        assert_eq!(
            tx_result,
            Err(InvalidTransaction::NonceTooHigh { tx: 42, state: 41 })
        )
    }

    #[test]
    /// Tx invalid if tx nonce < caller nonce.
    fn validation_nonce_too_low() {
        let tx_env = TxEnv {
            nonce: Some(40),
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            ..Default::default()
        };

        let mut db = Db::default();
        db.set_account(Address::default(), 41, U256::MAX, HashMap::default());

        let tx_result =
            env.validate_transaction(&db.get_account(Address::default()).unwrap().clone().into());

        assert_eq!(
            tx_result,
            Err(InvalidTransaction::NonceTooLow { tx: 40, state: 41 })
        )
    }

    #[test]
    fn tx_gas_limit_higher_than_block_gas_limit() {
        let tx_env = TxEnv {
            gas_limit: TX_BASE_COST + 999,
            ..Default::default()
        };

        let block_env = BlockEnv {
            gas_limit: U256::from(TX_BASE_COST + 998),
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            block: block_env,
            ..Default::default()
        };

        let tx_result = env.validate_transaction(&DbAccount::empty().into());

        assert_eq!(
            tx_result,
            Err(InvalidTransaction::CallerGasLimitMoreThanBlock)
        )
    }

    #[test]
    /// Tx invalid if gas limit < intrinsic cost
    fn tx_gas_limit_lower_than_intrinsic_cost() {
        let tx_env = TxEnv {
            gas_limit: 1,
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            ..Default::default()
        };

        let tx_result = env.validate_transaction(&DbAccount::empty().into());

        assert_eq!(
            tx_result,
            Err(InvalidTransaction::CallGasCostMoreThanGasLimit)
        )
    }

    #[test]
    /// Tx invalid if caller has deployed code
    fn tx_caller_with_code() {
        let caller_addr = H160::from_low_u64_be(40);

        let tx_env = TxEnv {
            caller: caller_addr,
            ..Default::default()
        };

        let block_env = BlockEnv {
            gas_limit: U256::MAX,
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            block: block_env,
            ..Default::default()
        };

        let mut db = Db::default();
        db.insert_contract(caller_addr, Bytecode::from("whatever"), U256::MAX);

        let tx_result =
            env.validate_transaction(&db.get_account(caller_addr).unwrap().clone().into());

        assert_eq!(tx_result, Err(InvalidTransaction::RejectCallerWithCode))
    }

    #[test]
    fn tx_max_priority_fee_greater_than_max_fee() {
        let tx_env = TxEnv {
            gas_priority_fee: Some(U256::from(101)),
            gas_price: U256::from(100),
            ..Default::default()
        };

        let block_env = BlockEnv {
            gas_limit: U256::MAX,
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            block: block_env,
            ..Default::default()
        };

        let tx_result = env.validate_transaction(&AccountInfo::default());

        assert_eq!(
            tx_result,
            Err(InvalidTransaction::PriorityFeeGreaterThanMaxFee)
        )
    }

    #[test]
    fn tx_max_fee_per_gas_lower_than_base_fee() {
        let tx_env = TxEnv {
            gas_price: 100.into(),
            ..Default::default()
        };

        let block_env = BlockEnv {
            gas_limit: U256::MAX,
            basefee: 101.into(),
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            block: block_env,
            ..Default::default()
        };

        let tx_result = env.validate_transaction(&AccountInfo::default());

        assert_eq!(tx_result, Err(InvalidTransaction::GasPriceLessThanBasefee))
    }

    #[test]
    /// Call tx with no data, no access list, should cost `TX_BASE_COST`
    fn intrinsic_cost_base() {
        let env = Env::default();
        let intrinsic_cost = env.calculate_intrinsic_cost();
        assert_eq!(intrinsic_cost, TX_BASE_COST)
    }

    #[test]
    /// Call tx with some data zero, no access list, should cost
    /// `TX_BASE_COST` + `TX_DATA_COST_PER_ZERO` * len(data)
    fn intrinsic_cost_data_zero() {
        let data = Bytes::from(vec![0, 0, 0, 0]);

        let tx_env = TxEnv {
            data: data.clone(),
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            ..Default::default()
        };

        let intrinsic_cost = env.calculate_intrinsic_cost();

        assert_eq!(
            intrinsic_cost,
            TX_BASE_COST + TX_DATA_COST_PER_ZERO * data.len() as u64
        )
    }

    #[test]
    /// Call tx with some data non zero, no access list, should cost
    /// `TX_BASE_COST` + `TX_DATA_COST_PER_NON_ZERO` * len(data)
    fn intrinsic_cost_data_non_zero() {
        let data = Bytes::from(vec![1, 2, 3, 4]);

        let tx_env = TxEnv {
            data: data.clone(),
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            ..Default::default()
        };

        let intrinsic_cost = env.calculate_intrinsic_cost();

        assert_eq!(
            intrinsic_cost,
            TX_BASE_COST + TX_DATA_COST_PER_NON_ZERO * data.len() as u64
        )
    }

    #[test]
    /// Call tx with some data zero and non zero, no access list, should cost
    /// `TX_BASE_COST`
    /// + `TX_DATA_COST_PER_ZERO` * len(data_zero)
    /// + `TX_DATA_COST_PER_NON_ZERO` * len(data_non_zero)
    fn intrinsic_cost_data_zero_non_zero() {
        let data_zero = vec![0, 0];
        let data_non_zero = vec![1, 3];
        let data = Bytes::from([data_zero.clone(), data_non_zero.clone()].concat());

        let tx_env = TxEnv {
            data: data.clone(),
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            ..Default::default()
        };

        let intrinsic_cost = env.calculate_intrinsic_cost();

        assert_eq!(
            intrinsic_cost,
            TX_BASE_COST
                + TX_DATA_COST_PER_NON_ZERO * data_non_zero.len() as u64
                + TX_DATA_COST_PER_ZERO * data_zero.len() as u64
        )
    }

    #[test]
    /// Call tx with no data, access list, should cost
    /// `TX_BASE_COST`
    /// + access_list_cost(access_list)
    fn intrinsic_cost_access_list() {
        let access_list: AccessList = vec![
            (
                H160::from_low_u64_be(40),
                vec![U256::from(1), U256::from(2)],
            ),
            (
                H160::from_low_u64_be(60),
                vec![U256::from(2), U256::from(3)],
            ),
        ];

        let tx_env = TxEnv {
            access_list: access_list.clone(),
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            ..Default::default()
        };

        let intrinsic_cost = env.calculate_intrinsic_cost();

        assert_eq!(
            intrinsic_cost,
            TX_BASE_COST + access_list_cost(&access_list)
        )
    }

    #[test]
    /// Call tx with some data, access list, should cost
    /// `TX_BASE_COST`
    /// + `TX_DATA_COST_PER_ZERO` * len(data_zero)
    /// + `TX_DATA_COST_PER_NON_ZERO` * len(data_non_zero)
    /// + access_list_cost(access_list)
    fn intrinsic_cost_data_access_list() {
        let data_zero = vec![0, 0];
        let data_non_zero = vec![1, 3];
        let data = Bytes::from([data_zero.clone(), data_non_zero.clone()].concat());

        let access_list: AccessList = vec![
            (
                H160::from_low_u64_be(40),
                vec![U256::from(1), U256::from(2)],
            ),
            (
                H160::from_low_u64_be(60),
                vec![U256::from(2), U256::from(3)],
            ),
        ];

        let tx_env = TxEnv {
            data: data.clone(),
            access_list: access_list.clone(),
            ..Default::default()
        };

        let env = Env {
            tx: tx_env,
            ..Default::default()
        };

        let intrinsic_cost = env.calculate_intrinsic_cost();

        assert_eq!(
            intrinsic_cost,
            TX_BASE_COST
                + TX_DATA_COST_PER_NON_ZERO * data_non_zero.len() as u64
                + TX_DATA_COST_PER_ZERO * data_zero.len() as u64
                + access_list_cost(&access_list)
        )
    }
}
