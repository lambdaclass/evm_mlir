use super::parser::parse_tests;
use std::path::PathBuf;

pub fn execute_tests(directory_path: PathBuf, verbose: bool) {
    parse_tests(directory_path)
        .iter()
        .for_each(|(path, _test)| {
            if verbose {
                println!("Running test: {}", path.display());
            }
            //TODO: Execute the test
        });
}
