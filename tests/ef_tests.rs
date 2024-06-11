use std::{collections::HashSet, path::Path};
mod ef_tests_executor;
use ef_tests_executor::models::TestSuite;
use evm_mlir::{program::Program, Env, Evm};

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
        // "eip3855_push0".into(),
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
        "ValueOverflow".into(),      // Invalid JSON
        "ValueOverflowParis".into(), // Invalid JSON
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
    let test: TestSuite = serde_json::from_reader(contents.as_bytes())
        .unwrap_or_else(|_| panic!("Failed to parse JSON test {}", path.display()));

    for (name, unit) in test.0 {
        eprintln!("Running test: {}", name);
        let Some(to) = unit.transaction.to else {
            return Err("`to` field is None".into());
        };
        let Some(account) = unit.pre.get(&to) else {
            return Err("Callee doesn't exist".into());
        };
        let code_len = account.code.len();
        eprintln!("Code len: {}", code_len);
        let env = Env::default();
        let program = Program::from_bytecode(&account.code)?;
        let evm = Evm::new(env, program);
        // // TODO: check the result
        let _result = evm.transact();
    }
    Ok(())
}

datatest_stable::harness!(run_test, "ethtests/GeneralStateTests/", r"^.*/*.json",);
