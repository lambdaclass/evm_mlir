use super::models::TestSuite;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use walkdir::{DirEntry, WalkDir};

const NOT_VALID_PATHS: [&str; 1] = [
    "tests/GeneralStateTests/Cancun/stEIP4844-blobtransactions", //
];

pub fn filter_json(entry: DirEntry) -> Option<DirEntry> {
    match entry.path().extension() {
        Some(ext) if "json" == ext => Some(entry),
        _ => None,
    }
}

pub fn filter_not_valid(entry: DirEntry) -> Option<DirEntry> {
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

pub fn parse_tests(directory_path: PathBuf) {
    for entry in WalkDir::new(directory_path)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| filter_not_valid(e))
        .filter_map(|e| filter_json(e))
    {
        println!("Processing file: {}", entry.path().display());
        let file = File::open(entry.path()).expect("Failed to open file");
        let reader = BufReader::new(file);
        let test_environment: TestSuite =
            serde_json::from_reader(reader).expect("Failed to parse JSON");
        println!("Parsed JSON: {:#?}", test_environment);
        break;
    }
}
