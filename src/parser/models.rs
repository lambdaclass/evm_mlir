use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/*
Models extracted from REVM
https://github.com/bluealloy/revm/blob/main/bins/revme/src/cmd/statetest/models/mod.rs
*/

pub type SpecName = String;

//Uncomment this line to import alloy_primitives (and comment type definitions down below)
// use alloy_primitives::{Address, Bytes, B256, U256};
//
pub type Address = String;
pub type Bytes = String;
pub type B256 = String;
pub type U256 = String;

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct TestSuite(pub BTreeMap<String, TestUnit>);

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestUnit {
    /// Test info is optional
    #[serde(default, rename = "_info")]
    pub info: Option<serde_json::Value>,
    pub env: Env,
    pub pre: HashMap<Address, AccountInfo>,
    pub post: BTreeMap<SpecName, Vec<Test>>,
    pub transaction: TransactionParts,
    #[serde(default)]
    pub out: Option<Bytes>,
}

/// State test indexed state result deserialization.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Test {
    pub expect_exception: Option<String>,
    pub indexes: TxPartIndices,
    pub hash: B256,
    #[serde(default)]
    pub post_state: HashMap<Address, AccountInfo>,
    pub logs: B256,
    pub txbytes: Option<Bytes>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TxPartIndices {
    pub data: usize,
    pub gas: usize,
    pub value: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountInfo {
    pub balance: U256,
    pub code: Bytes,
    pub nonce: String, //TODO: This should be u64
    pub storage: HashMap<U256, U256>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Env {
    pub current_coinbase: Address,
    pub current_difficulty: U256,
    pub current_gas_limit: U256,
    pub current_number: U256,
    pub current_timestamp: U256,
    pub current_base_fee: Option<U256>,
    pub previous_hash: Option<B256>,
    pub current_random: Option<B256>,
    pub current_beacon_root: Option<B256>,
    pub current_withdrawals_root: Option<B256>,
    pub parent_blob_gas_used: Option<U256>,
    pub parent_excess_blob_gas: Option<U256>,
    pub current_excess_blob_gas: Option<U256>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransactionParts {
    pub data: Vec<Bytes>,
    pub gas_limit: Vec<U256>,
    pub gas_price: Option<U256>,
    pub nonce: U256,
    pub secret_key: B256,
    /// if sender is not present we need to derive it from secret key.
    #[serde(default)]
    pub sender: Option<Address>,
    //#[serde(deserialize_with = "deserialize_maybe_empty")]
    pub to: Option<Address>,
    pub value: Vec<U256>,
    pub max_fee_per_gas: Option<U256>,
    pub max_priority_fee_per_gas: Option<U256>,
    #[serde(default)]
    pub access_lists: Vec<Option<AccessList>>,
    #[serde(default)]
    pub blob_versioned_hashes: Vec<B256>,
    pub max_fee_per_blob_gas: Option<U256>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessListItem {
    pub address: Address,
    pub storage_keys: Vec<B256>,
}

pub type AccessList = Vec<AccessListItem>;
