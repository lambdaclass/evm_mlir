use revm_comparison::run_with_evm_mlir;
use std::env;

fn main() {
    const PROGRAM: &str = "7f0000000000000000000000000000000000000000000000000000000000000080600260025b8215603b57906001018091029160019003916025565b9150505f5260205ff3";
    let runs = env::args().nth(1).unwrap();

    run_with_evm_mlir(PROGRAM, runs.parse().unwrap());
}
