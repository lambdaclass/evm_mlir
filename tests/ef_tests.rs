use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
mod ef_tests_executor;
use ef_tests_executor::models::{AccountInfo, TestSuite};
use evm_mlir::{db::Db, env::TransactTo, Env, Evm};

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
        "stEIP3855-push0".into(),
        "stEIP3860-limitmeterinitcode".into(),
        "stArgsZeroOneBalance".into(),
        "stRevertTest".into(),
        "eip3855_push0".into(),
        "eip4844_blobs".into(),
        "stZeroCallsRevert".into(),
        "stSStoreTest".into(),
        "stEIP2930".into(),
        "stRecursiveCreate".into(),
        "vmIOandFlowOperations".into(),
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
        "eip1344_chainid".into(),
        "vmBitwiseLogicOperation".into(),
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
        let Some(to) = unit.transaction.to else {
            return Err("`to` field is None".into());
        };
        let Some(account) = unit.pre.get(&to) else {
            return Err("Callee doesn't exist".into());
        };
        let Some(gas_price) = unit.transaction.gas_price else {
            // do we need to raise error here?
            return Err("`gas_price` field is None".into());
        };
        let Some(sender) = unit.transaction.sender else {
            // do we need to raise error here?
            return Err("`sender` field is None".into());
        };
        // Each TestUnit contains multiple tests (with different outcomes depending on the spec)

        // NOTE: currently we only support Cancun spec
        let Some(tests) = unit.post.get("Cancun") else {
            return Ok(());
        };
        for test in tests {
            let mut env = Env::default();
            env.tx.transact_to = TransactTo::Call(to);
            env.tx.gas_price = gas_price;
            env.tx.caller = sender;
            env.tx.gas_limit = unit.transaction.gas_limit[test.indexes.gas].as_u64();
            env.tx.value = unit.transaction.value[test.indexes.value];
            env.tx.data = unit.transaction.data[test.indexes.data].clone();

            env.block.number = unit.env.current_number;
            env.block.coinbase = unit.env.current_coinbase;
            env.block.timestamp = unit.env.current_timestamp;
            env.block.set_blob_base_fee(
                unit.env
                    .current_excess_blob_gas
                    .unwrap_or_default()
                    .as_u64(),
            );

            if let Some(basefee) = unit.env.current_base_fee {
                env.block.basefee = basefee;
            };
            let mut db = Db::new().with_bytecode(to, account.code.clone());

            // Load pre storage into db
            for (address, account_info) in unit.pre.iter() {
                db = db.with_bytecode(address.to_owned(), account_info.code.clone());
                db.update_account(
                    address.to_owned(),
                    account_info.nonce,
                    account_info.balance,
                    account_info.storage.clone(),
                );
            }
            let mut evm = Evm::new(env, db);

            let res = evm.transact().unwrap();

            if let Some(_e) = &test.expect_exception {
                assert!(!res.result.is_success());
                // TODO: check if returned error is the same?
            }

            assert!(res.result.is_success());
            assert_eq!(res.result.output().cloned(), unit.out);
            // TODO: check logs

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
    Ok(())
}

datatest_stable::harness!(run_test, "ethtests/GeneralStateTests/", r"^.*/*.json",);
