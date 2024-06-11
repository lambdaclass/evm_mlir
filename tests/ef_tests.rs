use std::path::PathBuf;
mod ef_tests_executor;
use ef_tests_executor::parser::parse_tests;

pub fn execute_tests(directory_path: PathBuf, verbose: bool) {
    parse_tests(directory_path)
        .iter()
        .for_each(|(path, _test)| {
            if verbose {
                println!("Running test: {}", path.display());
            }
        });
}
