use evm_mlir::{compile_binary, constants::REVERT_EXIT_CODE, opcodes::Operation};
use tempfile::NamedTempFile;

fn run_program_assert_result(program: Vec<Operation>, expected_result: u8) {
    let output_file = NamedTempFile::new()
        .expect("failed to generate tempfile")
        .into_temp_path();

    compile_binary(program, &output_file).expect("failed to compile program");

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

fn new_32_byte_immediate(value: u8) -> [u8; 32] {
    let mut arr = [0; 32];
    arr[31] = value;
    arr
}

#[test]
fn push32_once() {
    let the_answer = 42;
    let program = vec![Operation::Push32([the_answer; 32])];
    run_program_assert_result(program, the_answer);
}

#[test]
fn push32_twice() {
    let the_answer = 42;
    let program = vec![
        Operation::Push32([0; 32]),
        Operation::Push32([the_answer; 32]),
    ];
    run_program_assert_result(program, the_answer);
}

#[test]
fn push32_fill_stack() {
    let stack_top = 88;
    // Push 1024 times
    let program = vec![Operation::Push32([stack_top; 32]); 1024];

    run_program_assert_result(program, stack_top);
}

#[test]
fn push32_stack_overflow() {
    // Push 1025 times
    let program = vec![Operation::Push32([88; 32]); 1025];

    run_program_assert_revert(program);
}

#[test]
fn push_push_add() {
    let (a, b) = (11, 31);

    let program = vec![
        Operation::Push32(new_32_byte_immediate(a)),
        Operation::Push32(new_32_byte_immediate(b)),
        Operation::Add,
    ];
    run_program_assert_result(program, a + b);
}

#[test]
fn add_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Add]);
}

#[test]
fn push_push_normal_mul() {
    let (a, b) = (2, 42);

    let program = vec![
        Operation::Push32(new_32_byte_immediate(a)),
        Operation::Push32(new_32_byte_immediate(b)),
        Operation::Mul,
    ];
    run_program_assert_result(program, a * b);
}

#[test]
fn mul_wraps_result() {
    let a = [0xFF; 32];
    let program = vec![
        Operation::Push32(a),
        Operation::Push32(new_32_byte_immediate(2)),
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
    let (a, b) = (1, 2);
    let program = vec![
        Operation::Push32(new_32_byte_immediate(a)),
        Operation::Push32(new_32_byte_immediate(b)),
        Operation::Pop,
    ];
    run_program_assert_result(program, a);
}

#[test]
fn pop_with_stack_underflow() {
    // Pop with an empty stack
    let program = vec![Operation::Pop];
    run_program_assert_revert(program);
}

#[test]
fn test_gt_less_than() {
    let (a, b) = (9, 8);
    let program = vec![
        Operation::Push32(new_32_byte_immediate(a)),
        Operation::Push32(new_32_byte_immediate(b)),
        Operation::Gt,
    ];
    run_program_assert_result(program, 1);
}

#[test]
fn test_gt_greater_than() {
    let (a, b) = (10, 15);
    let program = vec![
        Operation::Push32(new_32_byte_immediate(a)),
        Operation::Push32(new_32_byte_immediate(b)),
        Operation::Gt,
    ];
    run_program_assert_result(program, 0);
}

#[test]
fn test_gt_equal() {
    let (a, b) = (10, 10);
    let program = vec![
        Operation::Push32(new_32_byte_immediate(a)),
        Operation::Push32(new_32_byte_immediate(b)),
        Operation::Gt,
    ];
    run_program_assert_result(program, 0);
}

#[test]
fn gt_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Gt]);
}
