use std::{error::Error, path::PathBuf, process::ExitCode};
mod ef_tests_executor;
use ef_tests_executor::{models::TestUnit, parser::parse_tests};
use libtest_mimic::{Arguments, Failed, Trial};

fn collect_tests() -> Vec<Trial> {
    let test_dirs = ["ethtests/GeneralStateTests/"];
    test_dirs
        .into_iter()
        .flat_map(parse_tests)
        .flat_map(|(path, suite)| {
            let path_str = get_kind_from_path(path);
            suite.0.into_iter().map(move |(name, unit)| {
                Trial::test(name, move || run_test(unit)).with_kind(path_str.clone())
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

fn run_test(_unit: TestUnit) -> Result<(), Failed> {
    Ok(())
}

fn main() -> Result<ExitCode, Box<dyn Error>> {
    let args = Arguments::from_args();
    let tests = collect_tests();
    Ok(libtest_mimic::run(&args, tests).exit_code())
}
