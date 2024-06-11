use revm_comparison::run_with_revm;
use std::env;

fn main() {
    const PROGRAM: &str = "7f0000000000000000000000000000000000000000000000000000000000000080600260025b8215603b57906001018091029160019003916025565b9150505f5260205ff3";
    let args: Vec<String> = env::args().collect();
    let runs = &args[1];

    run_with_revm(PROGRAM, runs.parse().unwrap());
}
