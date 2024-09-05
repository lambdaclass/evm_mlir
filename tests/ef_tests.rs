use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
mod ef_tests_executor;
use bytes::Bytes;
use ef_tests_executor::models::{AccountInfo, TestSuite};
use evm_mlir::{db::Db, env::TransactTo, result::ExecutionResult, Env, Evm};

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
        "stEIP3651-warmcoinbase".into(),
        "stArgsZeroOneBalance".into(),
        //"stTimeConsuming".into(), // this works, but it's REALLY is time consuming
        "stRevertTest".into(),
        "eip3855_push0".into(),
        "eip4844_blobs".into(),
        "stZeroCallsRevert".into(),
        "stEIP2930".into(),
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
        "stEIP150singleCodeGasPrices".into(),
        "stCreate2".into(),
        "stSpecialTest".into(),
        "stRecursiveCreate".into(),
        "vmIOandFlowOperations".into(),
        "stEIP150Specific".into(),
        "stExtCodeHash".into(),
        "stCallCodes".into(),
        "stRandom2".into(),
        "stMemoryStressTest".into(),
        "stStaticFlagEnabled".into(),
        "vmTests".into(),
        "stZeroKnowledge".into(),
        "stLogTests".into(),
        "stBugs".into(),
        "stEIP1559".into(),
        "stStaticCall".into(),
        "stMemExpandingEIP150Calls".into(),
        "stTransactionTest".into(),
        "eip3860_initcode".into(),
        "stCodeCopyTest".into(),
        "stPreCompiledContracts".into(),
        "stNonZeroCallsTest".into(),
        "stMemoryTest".into(),
        "stRandom".into(),
        "stInitCodeTest".into(),
        "stBadOpcode".into(),
        "eip1153_tstore".into(),
        "stSolidityTest".into(),
        "yul".into(),
        "stEIP3607".into(),
        "stCreateTest".into(),
        "eip198_modexp_precompile".into(),
        "stRefundTest".into(),
        "stZeroCallsTest".into(),
        "stAttackTest".into(),
        "eip2930_access_list".into(),
        "stExample".into(),
        "vmArithmeticTest".into(),
        "stQuadraticComplexityTest".into(),
        "stSelfBalance".into(),
        "stEIP3855-push0".into(),
        "stWalletTest".into(),
        "vmLogTest".into(),
    ])
}

fn get_ignored_suites() -> HashSet<String> {
    HashSet::from([
        "ValueOverflow".into(),      // TODO: parse bigint tx value
        "ValueOverflowParis".into(), // TODO: parse bigint tx value
    ])
}

fn convert_to_hex(account_info_code: Bytes) -> Bytes {
    let hex_string = std::str::from_utf8(&account_info_code[2..]).unwrap(); // we don't need the 0x
    let mut opcodes = Vec::new();
    for i in (0..hex_string.len()).step_by(2) {
        let pair = &hex_string[i..i + 2];
        let value = u8::from_str_radix(pair, 16).unwrap();
        opcodes.push(value);
    }
    Bytes::from(opcodes)
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
        let to = match unit.transaction.to {
            Some(to) => TransactTo::Call(to),
            None => TransactTo::Create,
        };

        let sender = unit.transaction.sender.unwrap_or_default();
        let gas_price = unit.transaction.gas_price.unwrap_or_default();

        for test in tests {
            let mut env = Env::default();
            env.tx.transact_to = to.clone();
            env.tx.gas_price = gas_price;
            env.tx.caller = sender;
            env.tx.gas_limit = unit.transaction.gas_limit[test.indexes.gas].as_u64();
            env.tx.value = unit.transaction.value[test.indexes.value];
            env.tx.data = convert_to_hex(unit.transaction.data[test.indexes.data].clone());

            env.block.number = unit.env.current_number;
            env.block.coinbase = unit.env.current_coinbase;
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
            let mut db = match to.clone() {
                TransactTo::Call(to) => {
                    let opcodes = convert_to_hex(unit.pre.get(&to).unwrap().code.clone());
                    Db::new().with_contract(to, opcodes)
                }
                TransactTo::Create => {
                    let opcodes =
                        convert_to_hex(unit.pre.get(&env.tx.caller).unwrap().code.clone());
                    Db::new().with_contract(env.tx.get_address(), opcodes)
                }
            };

            // Load pre storage into db
            for (address, account_info) in unit.pre.iter() {
                let opcodes = convert_to_hex(account_info.code.clone());
                db = db.with_contract(address.to_owned(), opcodes);
                db.set_account(
                    address.to_owned(),
                    account_info.nonce,
                    account_info.balance,
                    account_info.storage.clone(),
                );
            }
            let mut evm = Evm::new(env, db);

            let res = evm.transact().unwrap();

            match (&test.expect_exception, &res.result) {
                (None, _) => {
                    if let Some((expected_output, output)) =
                        unit.out.as_ref().zip(res.result.output())
                    {
                        if expected_output != output {
                            return Err("Wrong output".into());
                        }
                    }
                }
                (
                    Some(_),
                    ExecutionResult::Halt {
                        reason: _,
                        gas_used: _,
                    },
                ) => {
                    return Ok(()); //Halt and want an error
                }
                _ => {
                    return Err("Expected exception but got none".into());
                }
            }

            // TODO: use rlp and hash to check logs

            // Test the resulting storage is the same as the expected storage
            let mut result_state = HashMap::new();
            for address in test.post_state.keys() {
                let account = res.state.get(address).unwrap();
                let opcodes = convert_to_hex(account.info.code.clone().unwrap());
                result_state.insert(
                    address.to_owned(),
                    AccountInfo {
                        balance: account.info.balance,
                        code: opcodes,
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
