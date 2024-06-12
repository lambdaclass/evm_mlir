use revm_comparison::run_with_evm_mlir;
use std::env;

fn main() {
    const PROGRAM: &str = "7f00000000000000000000000000000000000000000000000000000000000003e75f60015b82156039578181019150909160019003916024565b9150505f5260205ff3";
    let runs = env::args().nth(1).unwrap();

    run_with_evm_mlir(PROGRAM, runs.parse().unwrap());
}
