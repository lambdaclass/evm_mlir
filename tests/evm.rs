use evm_mlir::{
    program::{Operation, Program},
    Env, Evm,
};
use num_bigint::BigUint;

fn get_fibonacci_program(n: u64) -> Vec<Operation> {
    vec![
        Operation::Push((32, n.into())),                  // 5 (needs to be >= 3)
        Operation::Push((1, BigUint::from(1_u8))), // fib(0)
        Operation::Push((1, BigUint::from(1_u8))), // fib(1)
        // 7

        // MAINLOOP:
        Operation::Jumpdest { pc: 37 },
        Operation::Dup(3),
        Operation::IsZero,
        Operation::Push((1, BigUint::ZERO + 28)), // CLEANUP
        Operation::Jumpi,
        // 13

        // fib step
        Operation::Dup(2),
        Operation::Dup(2),
        Operation::Add,
        Operation::Swap(2),
        Operation::Pop,
        Operation::Swap(1),
        // 19

        // decrement fib step counter
        Operation::Swap(2),
        Operation::Push((1, BigUint::from(1_u8))),
        Operation::Swap(1),
        Operation::Sub,
        Operation::Swap(2),
        // 25
        Operation::Push((1, BigUint::from(7_u8))), // goto MAINLOOP
        Operation::Jump,
        // 28

        // CLEANUP:
        Operation::Jumpdest {pc: ...},
        Operation::Swap(2),
        Operation::Pop,
        Operation::Pop,
        // Return the requested fibonacci element
        Operation::Push0,
        Operation::Mstore,
        Operation::Push((1, 32_u8.into())),
        Operation::Push0,
        Operation::Return,
    ]
}

#[test]
fn fibonacci_example() {
    let program = Program::from(get_fibonacci_program(100));
    let env = Env::default();
    let env = Evm::new(env, program);

    let result = env.transact();

    assert!(result.is_success());
    let number = BigUint::from_bytes_be(result.return_data().unwrap());
    println!("{number}");
}
