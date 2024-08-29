use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
mod ef_tests_executor;
use ef_tests_executor::models::{AccountInfo, TestSuite};
use ethereum_types::U256;
use evm_mlir::{
    db::Db,
    env::{AccessList, TransactTo},
    Env, Evm,
};

fn get_group_name_from_path(path: &Path) -> String {
    // Gets the parent directory's name.
    // Example: ethtests/GeneralStateTests/stArgsZeroOneBalance/addmodNonConst.json
    // -> stArgsZeroOneBalance
    path.ancestors()
        .into_iter()
        .nth(1)
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

fn get_suite_name_from_path(path: &Path) -> String {
    // Example: ethtests/GeneralStateTests/stArgsZeroOneBalance/addmodNonConst.json
    // -> addmodNonConst
    path.file_stem().unwrap().to_str().unwrap().to_string()
}

fn get_ignored_groups() -> HashSet<String> {
    HashSet::from([
        "stEIP4844-blobtransactions".into(),
        "stEIP5656-MCOPY".into(),
        "stEIP1153-transientStorage".into(),
        "stEIP3651-warmcoinbase".into(),
        "stEIP3860-limitmeterinitcode".into(),
        "stArgsZeroOneBalance".into(),
        "stRevertTest".into(),
        "eip3855_push0".into(),
        "eip4844_blobs".into(),
        "stZeroCallsRevert".into(),
        "stSStoreTest".into(),
        "stSystemOperationsTest".into(),
        "stReturnDataTest".into(),
        "vmPerformance".into(),
        "stHomesteadSpecific".into(),
        "stStackTests".into(),
        "eip5656_mcopy".into(),
        "eip6780_selfdestruct".into(),
        "stCallCreateCallCodeTest".into(),
        "stPreCompiledContracts2".into(),
        "stZeroKnowledge2".into(),
        "stDelegatecallTestHomestead".into(),
        "stTimeConsuming".into(),
        "stEIP150singleCodeGasPrices".into(),
        "stTransitionTest".into(),
        "stCreate2".into(),
        "stSpecialTest".into(),
        "stEIP150Specific".into(),
        "eip3651_warm_coinbase".into(),
        "stSLoadTest".into(),
        "stExtCodeHash".into(),
        "stCallCodes".into(),
        "stRandom2".into(),
        "stMemoryStressTest".into(),
        "stStaticFlagEnabled".into(),
        "vmTests".into(),
        "opcodes".into(),
        "stEIP158Specific".into(),
        "stZeroKnowledge".into(),
        "stShift".into(),
        "stLogTests".into(),
        "eip7516_blobgasfee".into(),
        "stBugs".into(),
        "stEIP1559".into(),
        "stSelfBalance".into(),
        "stStaticCall".into(),
        "stCallDelegateCodesHomestead".into(),
        "stMemExpandingEIP150Calls".into(),
        "stTransactionTest".into(),
        "eip3860_initcode".into(),
        "stCodeCopyTest".into(),
        "stPreCompiledContracts".into(),
        "stNonZeroCallsTest".into(),
        "stChainId".into(),
        "vmLogTest".into(),
        "stMemoryTest".into(),
        "stWalletTest".into(),
        "stRandom".into(),
        "stInitCodeTest".into(),
        "stBadOpcode".into(),
        "eip1153_tstore".into(),
        "stSolidityTest".into(),
        "stCallDelegateCodesCallCodeHomestead".into(),
        "yul".into(),
        "stEIP3607".into(),
        "stCreateTest".into(),
        "eip198_modexp_precompile".into(),
        "stCodeSizeLimit".into(),
        "stRefundTest".into(),
        "stZeroCallsTest".into(),
        "stAttackTest".into(),
        "eip2930_access_list".into(),
        "stExample".into(),
        "vmArithmeticTest".into(),
        "stQuadraticComplexityTest".into(),
    ])
}

fn get_ignored_suites() -> HashSet<String> {
    HashSet::from([
        "ValueOverflow".into(),      // TODO: parse bigint tx value
        "ValueOverflowParis".into(), // TODO: parse bigint tx value
    ])
}

fn run_test(path: &Path, contents: String) -> datatest_stable::Result<()> {
    let group_name = get_group_name_from_path(path);

    if get_ignored_groups().contains(&group_name) {
        return Ok(());
    }

    let suite_name = get_suite_name_from_path(path);

    if get_ignored_suites().contains(&suite_name) {
        return Ok(());
    }
    let test_suite: TestSuite = serde_json::from_reader(contents.as_bytes())
        .unwrap_or_else(|_| panic!("Failed to parse JSON test {}", path.display()));

    for (_name, unit) in test_suite.0 {
        // NOTE: currently we only support Cancun spec
        let Some(tests) = unit.post.get("Cancun") else {
            continue;
        };

        match unit.transaction.to {
            Some(to) => {
                let Some(account) = unit.pre.get(&to) else {
                    return Err("Callee doesn't exist".into());
                };
                let sender = unit.transaction.sender.unwrap_or_default();
                let gas_price = unit.transaction.gas_price.unwrap_or_default();

                for test in tests {
                    let mut env = Env::default();
                    env.tx.transact_to = TransactTo::Call(to);
                    env.tx.gas_price = gas_price;
                    env.tx.caller = sender;
                    env.tx.gas_limit = unit.transaction.gas_limit[test.indexes.gas].as_u64();
                    env.tx.value = unit.transaction.value[test.indexes.value];
                    env.tx.data = unit.transaction.data[test.indexes.data].clone();
                    let access_list_vector = if let Some(Some(access_list_vector)) = unit
                        .transaction
                        .access_lists
                        .get(test.indexes.data)
                        .cloned()
                    {
                        access_list_vector
                    } else {
                        vec![]
                    };

                    let mut access_list = AccessList::default();
                    for access_list_item in access_list_vector {
                        access_list.add_address(access_list_item.address);
                        for storage_key in access_list_item.storage_keys {
                            let storage_key = U256::from(storage_key.as_bytes());
                            access_list.add_storage(access_list_item.address, storage_key);
                        }
                    }

                    env.block.number = unit.env.current_number;
                    env.block.coinbase = unit.env.current_coinbase;
                    access_list.add_address(env.block.coinbase); // after Shanghai, coinbase address is added to access list
                    access_list.add_address(env.tx.caller); // after Berlin, tx.sender and tx.to is added to access list
                    access_list.add_address(to);
                    env.tx.access_list = access_list;
                    env.block.timestamp = unit.env.current_timestamp;
                    let excess_blob_gas = unit
                        .env
                        .current_excess_blob_gas
                        .unwrap_or_default()
                        .as_u64();
                    env.block.set_blob_base_fee(excess_blob_gas);

                    if let Some(basefee) = unit.env.current_base_fee {
                        env.block.basefee = basefee;
                    };
                    let mut db = Db::new().with_contract(to, account.code.clone());

                    // Load pre storage into db
                    for (address, account_info) in unit.pre.iter() {
                        db = db.with_contract(address.to_owned(), account_info.code.clone());
                        db.set_account(
                            address.to_owned(),
                            account_info.nonce,
                            account_info.balance,
                            account_info.storage.clone(),
                        );
                    }
                    let mut evm = Evm::new(env, db);

                    let res = evm.transact().unwrap();

                    if test.expect_exception.is_some() {
                        assert!(!res.result.is_success());
                        // NOTE: the expect_exception string is an error description, we don't check the expected error
                        continue;
                    }

                    assert!(res.result.is_success());
                    assert_eq!(res.result.output().cloned(), unit.out);

                    // TODO: use rlp and hash to check logs

                    // Test the resulting storage is the same as the expected storage
                    let mut result_state = HashMap::new();
                    for address in test.post_state.keys() {
                        let account = res.state.get(address).unwrap();
                        result_state.insert(
                            address.to_owned(),
                            AccountInfo {
                                balance: account.info.balance,
                                code: account.info.code.clone().unwrap(),
                                nonce: account.info.nonce,
                                storage: account
                                    .storage
                                    .clone()
                                    .into_iter()
                                    .map(|(addr, slot)| (addr, slot.present_value))
                                    .collect(),
                            },
                        );
                    }
                    assert_eq!(test.post_state, result_state);
                }
            }
            None => {
                let sender = unit.transaction.sender.unwrap_or_default();
                let gas_price = unit.transaction.gas_price.unwrap_or_default();

                for test in tests {
                    let mut env = Env::default();
                    env.tx.transact_to = TransactTo::Create;
                    env.tx.gas_price = gas_price;
                    env.tx.caller = sender;
                    env.tx.gas_limit = unit.transaction.gas_limit[test.indexes.gas].as_u64();
                    env.tx.value = unit.transaction.value[test.indexes.value];
                    env.tx.data = unit.transaction.data[test.indexes.data].clone();
                    let mut access_list = AccessList::default();
                    env.block.number = unit.env.current_number;
                    env.block.coinbase = unit.env.current_coinbase;
                    access_list.add_address(env.block.coinbase); // after Shanghai, coinbase address is added to access list
                    access_list.add_address(env.tx.caller); // after Berlin, tx.sender and tx.to is added to access list
                    env.tx.access_list = access_list;
                    env.block.timestamp = unit.env.current_timestamp;
                    let excess_blob_gas = unit
                        .env
                        .current_excess_blob_gas
                        .unwrap_or_default()
                        .as_u64();
                    env.block.set_blob_base_fee(excess_blob_gas);

                    if let Some(basefee) = unit.env.current_base_fee {
                        env.block.basefee = basefee;
                    };
                    let mut db = Db::new();

                    // Load pre storage into db
                    for (address, account_info) in unit.pre.iter() {
                        db = db.with_contract(address.to_owned(), account_info.code.clone());
                        db.set_account(
                            address.to_owned(),
                            account_info.nonce,
                            account_info.balance,
                            account_info.storage.clone(),
                        );
                    }
                    let mut evm = Evm::new(env, db);

                    let res = evm.transact().unwrap();

                    if test.expect_exception.is_some() {
                        assert!(!res.result.is_success());
                        // NOTE: the expect_exception string is an error description, we don't check the expected error
                        continue;
                    }

                    assert!(res.result.is_success());
                    assert_eq!(res.result.output().cloned(), unit.out);

                    // TODO: use rlp and hash to check logs

                    // Test the resulting storage is the same as the expected storage
                    let mut result_state = HashMap::new();
                    for address in test.post_state.keys() {
                        let account = res.state.get(address).unwrap();
                        result_state.insert(
                            address.to_owned(),
                            AccountInfo {
                                balance: account.info.balance,
                                code: account.info.code.clone().unwrap(),
                                nonce: account.info.nonce,
                                storage: account
                                    .storage
                                    .clone()
                                    .into_iter()
                                    .map(|(addr, slot)| (addr, slot.present_value))
                                    .collect(),
                            },
                        );
                    }
                    assert_eq!(test.post_state, result_state);
                }
            }
        }
    }
    Ok(())
}

datatest_stable::harness!(run_test, "ethtests/GeneralStateTests", r"^.*/*.json",);
