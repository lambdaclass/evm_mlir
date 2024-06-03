use evm_mlir::{compile_binary, program::{self, Program,Operation}};
use num_bigint::BigUint;
//use melior::ir::Operation;

fn main() {
    // let args: Vec<String> = std::env::args().collect();
    // let path = args.get(1).expect("No path provided").as_str();
    // let bytecode = std::fs::read(path).expect("Could not read file");
    // let program = Program::from_bytecode(&bytecode);
    // let output_file = "output";

    // compile_binary(&program, output_file).unwrap();
    // println!("Done!");
    // println!("Program was compiled in {output_file}");
    let program_vec = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Add,
    ];
    let program = Program::from(program_vec);
    let output_file = "output";
    compile_binary(&program, output_file).unwrap();
}
