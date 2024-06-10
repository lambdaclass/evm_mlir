use std::{collections::HashSet, error::Error, path::PathBuf, process::ExitCode, sync::Arc};
mod ef_tests_executor;
use ef_tests_executor::{models::TestUnit, parser::parse_tests};
use evm_mlir::{program::Program, Env, Evm};
use libtest_mimic::{Arguments, Failed, Trial};

fn collect_tests() -> Vec<Trial> {
    let test_dirs = ["ethtests/GeneralStateTests/"];
    let ignored_suites_arc = Arc::new(get_ignored_suites());
    let ignored_tests_arc = Arc::new(get_ignored_tests());
    test_dirs
        .into_iter()
        .flat_map(parse_tests)
        .flat_map(|(path, suite)| {
            let ignored_suites = ignored_suites_arc.clone();
            let ignored_tests = ignored_tests_arc.clone();
            let path_str = get_kind_from_path(path);
            suite.0.into_iter().map(move |(name, unit)| {
                let ignored_flag =
                    ignored_suites.contains(&path_str) || ignored_tests.contains(&name);
                Trial::test(name.clone(), move || run_test(name, unit))
                    .with_kind(path_str.clone())
                    .with_ignored_flag(ignored_flag)
            })
        })
        .collect()
}

fn get_kind_from_path(path: PathBuf) -> String {
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

fn get_ignored_suites() -> HashSet<String> {
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

fn get_ignored_tests() -> HashSet<String> {
    HashSet::from([])
}

fn run_test(name: String, unit: TestUnit) -> Result<(), Failed> {
    println!("{name}");
    let env = Env::default();
    let Some(to) = unit.transaction.to else {
        return Err(Failed::from("`to` field is None"));
    };
    let Some(account) = unit.pre.get(&to) else {
        return Err(Failed::from("Callee doesn't exist"));
    };
    let program = Program::from_bytecode(&account.code).map_err(Failed::from)?;
    let evm = Evm::new(env, program);
    // TODO: check the result
    let _result = evm.transact();
    Ok(())
}

fn main() -> Result<ExitCode, Box<dyn Error>> {
    let args = Arguments::from_args();
    let tests = collect_tests();
    Ok(libtest_mimic::run(&args, tests).exit_code())
}
