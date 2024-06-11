use revm_comparison::run_with_revm;
use std::env;

fn main() {
    const PROGRAM: &str = "7f00000000000000000000000000000000000000000000000000000000000003e75f60015b82156039578181019150909160019003916024565b9150505f5260205ff3";
    let args: Vec<String> = env::args().collect();
    let runs = &args[1];

    run_with_revm(PROGRAM, runs.parse().unwrap());
}
