use evm_mlir::{
    compile_binary,
    constants::REVERT_EXIT_CODE,
    program::{Operation, Program},
};
use num_bigint::BigUint;
use tempfile::NamedTempFile;

fn run_program_assert_result(operations: Vec<Operation>, expected_result: u8) {
    let program = Program::from(operations);
    let output_file = NamedTempFile::new()
        .expect("failed to generate tempfile")
        .into_temp_path();

    compile_binary(&program, &output_file).expect("failed to compile program");

    assert!(output_file.exists(), "output file does not exist");

    let mut res = std::process::Command::new(&output_file)
        .spawn()
        .expect("spawn process failed");
    let output = res.wait().expect("wait for process failed");

    assert_eq!(output.code().expect("no exit code"), expected_result.into());
}

fn run_program_assert_revert(program: Vec<Operation>) {
    // TODO: design a way to check for stack overflow
    run_program_assert_result(program, REVERT_EXIT_CODE);
}

#[test]
fn push_once() {
    let value = BigUint::from(5_u8);

    // For PUSH0
    let program = vec![Operation::Push(BigUint::ZERO)];
    run_program_assert_result(program, 0);

    // For PUSH1, ... , PUSH32
    for i in 0..32 {
        let shifted_value: BigUint = value.clone() << (i * 8);
        let program = vec![Operation::Push(shifted_value.clone())];
        let expected_result: u8 = (shifted_value % 256_u32).try_into().unwrap();
        run_program_assert_result(program, expected_result);
    }
}

#[test]
fn push_twice() {
    let the_answer = BigUint::from(42_u8);

    let program = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(the_answer.clone()),
    ];
    run_program_assert_result(program, the_answer.try_into().unwrap());
}

#[test]
fn push_fill_stack() {
    let stack_top = BigUint::from(88_u8);

    // Push 1024 times
    let program = vec![Operation::Push(stack_top.clone()); 1024];
    run_program_assert_result(program, stack_top.try_into().unwrap());
}

#[test]
fn push_stack_overflow() {
    // Push 1025 times
    let program = vec![Operation::Push(BigUint::from(88_u8)); 1025];
    run_program_assert_revert(program);
}

#[test]
fn push_push_add() {
    let (a, b) = (BigUint::from(11_u8), BigUint::from(31_u8));

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Add,
    ];
    run_program_assert_result(program, (a + b).try_into().unwrap());
}

#[test]
fn push_push_sub() {
    let (a, b) = (BigUint::from(11_u8), BigUint::from(31_u8));

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Sub,
    ];
    run_program_assert_result(program, 20);
}

#[test]
fn substraction_wraps_the_result() {
    let (a, b) = (BigUint::from(10_u8), BigUint::from(0_u8));

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Sub,
    ];

    let result = 0_u8.wrapping_sub(10);

    run_program_assert_result(program, result);
}

#[test]
fn sub_add_wrapping() {
    let a = (BigUint::from(1_u8) << 256) - 1_u8;

    let program = vec![
        Operation::Push(a),
        Operation::Push(BigUint::from(10_u8)),
        Operation::Add,
        Operation::Push(BigUint::from(10_u8)),
        Operation::Sub,
    ];

    run_program_assert_result(program, 1);
}

#[test]
fn add_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Add]);
}

#[test]
fn div_without_remainder() {
    let (a, b) = (BigUint::from(20_u8), BigUint::from(5_u8));

    let expected_result = (&a / &b).try_into().unwrap();

    let program = vec![
        Operation::Push(b), //
        Operation::Push(a), //
        Operation::Div,
    ];

    run_program_assert_result(program, expected_result);
}

#[test]
fn div_signed_division() {
    // a = [1, 0, 0, 0, .... , 0, 0, 0, 0] == 1 << 255
    let mut a = BigUint::from(0_u8);
    a.set_bit(255, true);
    // b = [0, 0, 1, 0, .... , 0, 0, 0, 0] == 1 << 253
    let mut b = BigUint::from(0_u8);
    b.set_bit(253, true);

    //r = a / b = [0, 0, 0, 0, ....., 0, 1, 0, 0] = 4 in decimal
    //If we take the lowest byte
    //r = [0, 0, 0, 0, 0, 1, 0, 0] = 4 in decimal
    let expected_result = (&a / &b).try_into().unwrap();

    let program = vec![
        Operation::Push(b), //
        Operation::Push(a), //
        Operation::Div,     //
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn div_with_remainder() {
    let (a, b) = (BigUint::from(21_u8), BigUint::from(5_u8));

    let expected_result = (&a / &b).try_into().unwrap();

    let program = vec![
        Operation::Push(b), //
        Operation::Push(a), //
        Operation::Div,
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn div_with_zero_denominator() {
    let (a, b) = (BigUint::from(5_u8), BigUint::from(0_u8));

    let expected_result: u8 = 0_u8;

    let program = vec![
        Operation::Push(b), //
        Operation::Push(a), //
        Operation::Div,
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn div_with_zero_numerator() {
    let (a, b) = (BigUint::from(0_u8), BigUint::from(10_u8));

    let expected_result = (&a / &b).try_into().unwrap();

    let program = vec![
        Operation::Push(b), //
        Operation::Push(a), //
        Operation::Div,
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn div_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Div]);
}

#[test]
fn push_push_normal_mul() {
    let (a, b) = (BigUint::from(2_u8), BigUint::from(42_u8));

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Mul,
    ];
    run_program_assert_result(program, (a * b).try_into().unwrap());
}

#[test]
fn mul_wraps_result() {
    let a = BigUint::from_bytes_be(&[0xFF; 32]);
    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Mul,
    ];
    run_program_assert_result(program, 254);
}

#[test]
fn mul_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Mul]);
}

#[test]
fn push_push_pop() {
    // Push two values to the stack and then pop once
    // The program result should be equal to the first
    // pushed value
    let (a, b) = (BigUint::from(1_u8), BigUint::from(2_u8));

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b),
        Operation::Pop,
    ];
    run_program_assert_result(program, a.try_into().unwrap());
}

#[test]
fn pop_with_stack_underflow() {
    // Pop with an empty stack
    let program = vec![Operation::Pop];
    run_program_assert_revert(program);
}

#[test]
fn push_push_byte() {
    let mut value: [u8; 32] = [0; 32];
    let desired_byte = 0xff;
    let offset: u8 = 16;
    value[offset as usize] = desired_byte;
    let value: BigUint = BigUint::from_bytes_be(&value);
    let program = vec![
        Operation::Push(value),
        Operation::Push(BigUint::from(offset)),
        Operation::Byte,
    ];
    run_program_assert_result(program, desired_byte);
}

#[test]
fn byte_with_stack_underflow() {
    let program = vec![Operation::Byte];
    run_program_assert_revert(program);
}

#[test]
fn byte_with_offset_out_of_bounds() {
    // must consider this case yet
    let value: [u8; 32] = [0xff; 32];
    let value: BigUint = BigUint::from_bytes_be(&value);
    let offset = BigUint::from(32_u8);
    let program = vec![
        Operation::Push(value),
        Operation::Push(offset),
        Operation::Byte,
    ];
    run_program_assert_result(program, 0);
}

#[test]
fn jumpdest() {
    let expected = 5;
    let program = vec![
        Operation::Jumpdest { pc: 0 },
        Operation::Push(BigUint::from(expected)),
        Operation::Jumpdest { pc: 34 },
    ];
    run_program_assert_result(program, expected)
}

#[test]
fn mod_with_non_zero_result() {
    let (num, den) = (BigUint::from(31_u8), BigUint::from(10_u8));
    let expected_result = (&num % &den).try_into().unwrap();

    let program = vec![Operation::Push(den), Operation::Push(num), Operation::Mod];
    run_program_assert_result(program, expected_result);
}

#[test]
fn mod_with_result_zero() {
    let (num, den) = (BigUint::from(10_u8), BigUint::from(2_u8));
    let expected_result = (&num % &den).try_into().unwrap();

    let program = vec![Operation::Push(den), Operation::Push(num), Operation::Mod];
    run_program_assert_result(program, expected_result);
}

#[test]
fn mod_with_zero_denominator() {
    let (num, den) = (BigUint::from(10_u8), BigUint::from(0_u8));

    let program = vec![Operation::Push(den), Operation::Push(num), Operation::Mod];
    run_program_assert_result(program, 0);
}

#[test]
fn mod_with_zero_numerator() {
    let (num, den) = (BigUint::from(0_u8), BigUint::from(25_u8));

    let program = vec![Operation::Push(den), Operation::Push(num), Operation::Mod];
    run_program_assert_result(program, 0);
}

#[test]
fn mod_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Mod]);
}

#[test]
fn addmod_with_non_zero_result() {
    let (a, b, den) = (
        BigUint::from(13_u8),
        BigUint::from(30_u8),
        BigUint::from(10_u8),
    );

    let program = vec![
        Operation::Push(den.clone()),
        Operation::Push(b.clone()),
        Operation::Push(a.clone()),
        Operation::Addmod,
    ];
    run_program_assert_result(program, ((a + b) % den).try_into().unwrap());
}

#[test]
fn addmod_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Addmod]);
}

#[test]
fn addmod_with_zero_denominator() {
    let program = vec![
        Operation::Push(BigUint::from(0_u8)),
        Operation::Push(BigUint::from(31_u8)),
        Operation::Push(BigUint::from(11_u8)),
        Operation::Addmod,
    ];
    run_program_assert_result(program, 0);
}
#[test]
fn push_push_normal_not() {
    let (a) = BigUint::from(1_u8);

    let program = vec![Operation::Push(a.clone()), Operation::Not];
    run_program_assert_result(program, (0).try_into().unwrap());
}
