use revm_comparison::run_with_revm;
use std::env;

fn main() {
    const PROGRAM: &str =
        "5f35600260025b8215601c57906001018091029160019003916006565b9150505f5260205ff3";
    let runs = env::args().nth(1).unwrap();

    run_with_revm(PROGRAM, runs.parse().unwrap(), 7_u32);
}
