use super::models::TestSuite;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use walkdir::{DirEntry, WalkDir};

const NOT_VALID_PATHS: [&str; 1] = [
    "tests/GeneralStateTests/Cancun/stEIP4844-blobtransactions", //
];

fn filter_json(entry: DirEntry) -> Option<DirEntry> {
    match entry.path().extension() {
        Some(ext) if "json" == ext => Some(entry),
        _ => None,
    }
}

fn filter_not_valid(entry: DirEntry) -> Option<DirEntry> {
    match entry.path().to_str() {
        Some(path) => {
            let filtered = NOT_VALID_PATHS.iter().any(|x| path.contains(*x));
            if filtered {
                None
            } else {
                Some(entry)
            }
        }
        _ => None,
    }
}

fn parse_test_suite(entry: DirEntry) -> (PathBuf, TestSuite) {
    let file = File::open(entry.path())
        .unwrap_or_else(|_| panic!("Failed to open file {}", entry.path().display()));
    let reader = BufReader::new(file);
    let test: TestSuite = serde_json::from_reader(reader)
        .unwrap_or_else(|_| panic!("Failed to parse JSON test {}", entry.path().display()));
    (PathBuf::from(entry.path()), test)
}

pub fn parse_tests(directory_path: PathBuf) -> Vec<(PathBuf, TestSuite)> {
    WalkDir::new(directory_path)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(filter_not_valid)
        .filter_map(filter_json)
        .map(parse_test_suite)
        .collect()
}
