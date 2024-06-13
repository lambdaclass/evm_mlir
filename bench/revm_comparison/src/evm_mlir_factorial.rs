use revm_comparison::run_with_evm_mlir;
use std::env;

fn main() {
    const PROGRAM: &str =
        "5f35600260025b8215601c57906001018091029160019003916006565b9150505f5260205ff3";
    let runs = env::args().nth(1).unwrap();
    let number_of_iterations = env::args().nth(2).unwrap();

    run_with_evm_mlir(
        PROGRAM,
        runs.parse().unwrap(),
        number_of_iterations.parse().unwrap(),
    );
    // NOTE: for really big numbers the result is zero due to
    // one every two iterations involving an even number.
}
