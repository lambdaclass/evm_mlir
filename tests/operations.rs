use evm_mlir::{
    constants::{gas_cost, REVERT_EXIT_CODE},
    context::Context,
    executor::Executor,
    program::{Operation, Program},
    syscall::SyscallContext,
};
use num_bigint::{BigInt, BigUint};
use rstest::rstest;
use tempfile::NamedTempFile;

fn run_program_assert_result_with_gas(
    operations: Vec<Operation>,
    expected_result: u8,
    initial_gas: u64,
) {
    let program = Program::from(operations);
    let output_file = NamedTempFile::new()
        .expect("failed to generate tempfile")
        .into_temp_path();

    let context = Context::new();
    let module = context
        .compile(&program, &output_file)
        .expect("failed to compile program");

    let executor = Executor::new(&module);

    let mut context = SyscallContext::default();

    let result = executor.execute(&mut context, initial_gas);

    assert_eq!(result, expected_result);
}

fn run_program_assert_result(operations: Vec<Operation>, expected_result: u8) {
    run_program_assert_result_with_gas(operations, expected_result, 1e7 as _);
}

fn run_program_assert_reverts_with_gas(program: Vec<Operation>, initial_gas: u64) {
    // TODO: design a way to check for stack overflow
    run_program_assert_result_with_gas(program, REVERT_EXIT_CODE, initial_gas);
}

fn run_program_assert_gas_exact(program: Vec<Operation>, expected_result: u8, exact_gas: u64) {
    run_program_assert_result_with_gas(program.clone(), expected_result, exact_gas);
    run_program_assert_reverts_with_gas(program, exact_gas - 1);
}

fn run_program_assert_revert(program: Vec<Operation>) {
    // TODO: design a way to check for stack overflow
    run_program_assert_result(program, REVERT_EXIT_CODE);
}

pub fn biguint_256_from_bigint(value: BigInt) -> BigUint {
    if value >= BigInt::ZERO {
        value.magnitude().clone()
    } else {
        let bytes = value.to_signed_bytes_be();
        let mut buffer = vec![255_u8; 32];
        let finish = 32;
        let start = finish - bytes.len();
        buffer[start..finish].copy_from_slice(&bytes);
        BigUint::from_bytes_be(&buffer)
    }
}

#[test]
fn push_once() {
    let value = BigUint::from(5_u8);

    // For PUSH0
    let program = vec![Operation::Push0];
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
fn push_reverts_without_gas() {
    let stack_top = 88_u8;
    let initial_gas = (gas_cost::PUSH0 + gas_cost::PUSHN) as _;

    let program = vec![Operation::Push0, Operation::Push(BigUint::from(stack_top))];
    run_program_assert_gas_exact(program, stack_top, initial_gas);
}

#[test]
fn push_stack_overflow() {
    // Push 1025 times
    let program = vec![Operation::Push(BigUint::from(88_u8)); 1025];
    run_program_assert_revert(program);
}

#[test]
fn dup1_once() {
    let program = vec![
        Operation::Push(BigUint::from(10_u8)),
        Operation::Push(BigUint::from(31_u8)),
        Operation::Dup(1),
        Operation::Pop,
    ];

    run_program_assert_result(program, 31);
}

#[test]
fn dup2_once() {
    let program = vec![
        Operation::Push(BigUint::from(4_u8)),
        Operation::Push(BigUint::from(5_u8)),
        Operation::Push(BigUint::from(6_u8)),
        Operation::Dup(2),
    ];

    run_program_assert_result(program, 5);
}

#[rstest]
#[case(1)]
#[case(2)]
#[case(3)]
#[case(4)]
#[case(5)]
#[case(6)]
#[case(7)]
#[case(8)]
#[case(9)]
#[case(10)]
#[case(11)]
#[case(12)]
#[case(13)]
#[case(14)]
#[case(15)]
#[case(16)]
fn dup_nth(#[case] nth: u8) {
    let iter = (0..16u8).rev().map(|x| Operation::Push(BigUint::from(x)));
    let mut program = Vec::from_iter(iter);

    program.push(Operation::Dup(nth.into()));

    run_program_assert_result(program, nth - 1);
}

#[test]
fn dup_with_stack_underflow() {
    let program = vec![Operation::Dup(1)];

    run_program_assert_revert(program);
}

#[test]
fn push_push_shl() {
    let program = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(BigUint::from(4_u8)),
        Operation::Shl,
    ];

    run_program_assert_result(program, 16);
}

#[test]
fn shl_shift_grater_than_255() {
    let program = vec![
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(256_u16)),
        Operation::Shl,
    ];

    run_program_assert_result(program, 0);
}

#[test]
fn shl_with_stack_underflow() {
    let program = vec![Operation::Shl];

    run_program_assert_revert(program);
}

#[test]
fn shl_out_of_gas() {
    let program = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(BigUint::from(4_u8)),
        Operation::Shl,
    ];
    let gas_needed = gas_cost::PUSHN * 2 + gas_cost::SHL;

    run_program_assert_gas_exact(program, 16, gas_needed as _);
}

#[test]
fn swap_first() {
    let program = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Swap(1),
    ];

    run_program_assert_result(program, 1);
}

#[test]
fn swap_16_and_get_the_swapped_one() {
    let program = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(3_u8)),
        Operation::Swap(16),
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
        Operation::Pop,
    ];

    run_program_assert_result(program, 3);
}

#[test]
fn swap_stack_underflow() {
    let program = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Swap(2),
    ];

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
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
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
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Div,     // <No collapse>
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn div_with_remainder() {
    let (a, b) = (BigUint::from(21_u8), BigUint::from(5_u8));

    let expected_result = (&a / &b).try_into().unwrap();

    let program = vec![
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Div,
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn div_with_zero_denominator() {
    let (a, b) = (BigUint::from(5_u8), BigUint::from(0_u8));

    let expected_result: u8 = 0_u8;

    let program = vec![
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Div,
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn div_with_zero_numerator() {
    let (a, b) = (BigUint::from(0_u8), BigUint::from(10_u8));

    let expected_result = (&a / &b).try_into().unwrap();

    let program = vec![
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Div,
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn div_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Div]);
}

#[test]
fn div_gas_should_revert() {
    let (a, b) = (BigUint::from(21_u8), BigUint::from(5_u8));

    let expected_result = (&a / &b).try_into().unwrap();

    let program = vec![
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Div,
    ];

    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::DIV;

    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn sdiv_without_remainder() {
    let (a, b) = (BigUint::from(20_u8), BigUint::from(5_u8));

    let expected_result = (&a / &b).try_into().unwrap();

    let program = vec![
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Sdiv,
    ];

    run_program_assert_result(program, expected_result);
}

#[test]
fn sdiv_signed_division_1() {
    // a = [1, 0, 0, 0, .... , 0, 0, 0, 0] == 1 << 255
    let mut a = BigUint::from(0_u8);
    a.set_bit(255, true);
    // b = [0, 0, 1, 0, .... , 0, 0, 0, 0] == 1 << 253
    let mut b = BigUint::from(0_u8);
    b.set_bit(253, true);

    //r = a / b = [1, 1, 1, 1, ....., 1, 1, 0, 0]
    //If we take the lowest byte
    //r = [1, 1, 1, 1, 1, 1, 0, 0] = 252 in decimal
    let expected_result: u8 = 252_u8;

    let program = vec![
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Sdiv,    // <No collapse>
    ];

    run_program_assert_result(program, expected_result);
}

#[test]
fn sdiv_signed_division_2() {
    let a = BigInt::from(-2_i8);
    let b = BigInt::from(-1_i8);

    let expected_result: u8 = (&a / &b).try_into().unwrap();

    let a_biguint = biguint_256_from_bigint(a);
    let b_biguint = biguint_256_from_bigint(b);

    let program = vec![
        Operation::Push(b_biguint), // <No collapse>
        Operation::Push(a_biguint), // <No collapse>
        Operation::Sdiv,            // <No collapse>
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn sdiv_with_remainder() {
    let (a, b) = (BigUint::from(21_u8), BigUint::from(5_u8));

    let expected_result = (&a / &b).try_into().unwrap();

    let program = vec![
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Sdiv,
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn sdiv_with_zero_denominator() {
    let (a, b) = (BigUint::from(5_u8), BigUint::from(0_u8));

    let expected_result: u8 = 0_u8;

    let program = vec![
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Sdiv,
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn sdiv_with_zero_numerator() {
    let (a, b) = (BigUint::from(0_u8), BigUint::from(10_u8));

    let expected_result = (&a / &b).try_into().unwrap();

    let program = vec![
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Sdiv,
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn sdiv_gas_should_revert() {
    let (a, b) = (2_u8, 10_u8);

    let program = vec![
        Operation::Push(BigUint::from(b)),
        Operation::Push(BigUint::from(a)),
        Operation::Sdiv,
    ];
    let initial_gas = gas_cost::PUSHN * 2 + gas_cost::SDIV;
    let expected_result = a / b;
    run_program_assert_gas_exact(program, expected_result, initial_gas as _);
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
fn mul_gas_should_revert() {
    let (a, b) = (BigUint::from(1_u8), BigUint::from(2_u8));
    let expected_result = (&a * &b).try_into().unwrap();
    let program = vec![
        Operation::Push(b), // <No collapse>
        Operation::Push(a), // <No collapse>
        Operation::Mul,     // <No collapse>
    ];

    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::MUL;
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn push_push_shr() {
    let program = vec![
        Operation::Push(BigUint::from(32_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Shr,
    ];

    run_program_assert_result(program, 8);
}

#[test]
fn shift_bigger_than_256() {
    let program = vec![
        Operation::Push(BigUint::from(255_u8)),
        Operation::Push(BigUint::from(256_u16)),
        Operation::Shr,
    ];

    run_program_assert_result(program, 0);
}

#[test]
fn shr_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Shr]);
}

#[test]
fn push_push_xor() {
    let program = vec![
        Operation::Push(BigUint::from(10_u8)),
        Operation::Push(BigUint::from(5_u8)),
        Operation::Xor,
    ];

    run_program_assert_result(program, 15);
}

#[test]
fn xor_with_stack_underflow() {
    let program = vec![Operation::Xor];

    run_program_assert_revert(program);
}

#[test]
fn xor_out_of_gas() {
    let (a, b) = (1_u8, 2_u8);
    let program = vec![
        Operation::Push(BigUint::from(a)),
        Operation::Push(BigUint::from(b)),
        Operation::Xor,
    ];
    let initial_gas = gas_cost::PUSHN * 2 + gas_cost::XOR;
    let expected_result = a ^ b;
    run_program_assert_gas_exact(program, expected_result, initial_gas as _);
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
fn push_push_sar() {
    let (value, shift) = (2_u8, 1_u8);
    let program = vec![
        Operation::Push(BigUint::from(value)),
        Operation::Push(BigUint::from(shift)),
        Operation::Sar,
    ];
    let expected_result = value >> shift;
    run_program_assert_result(program, expected_result);
}

#[test]
fn sar_with_stack_underflow() {
    let program = vec![Operation::Sar];
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
fn sar_with_negative_value_preserves_sign() {
    // in this example the the value to be shifted is a 256 bit number
    // where the most significative bit is 1 cand the rest of the bits are 0.
    // i.e,  value = 1000..0000
    //
    // if we shift this value 255 positions to the right, given that
    // the sar operation preserves the sign, the result must be a number
    // in which every bit is 1
    // i.e, result = 1111..1111
    //
    // given that the program results is a u8, the result is then truncated
    // to the less 8 significative bits, i.e  result = 0b11111111.
    //
    // this same example can be visualized in the evm playground in the following link
    // https://www.evm.codes/playground?fork=cancun&unit=Wei&codeType=Mnemonic&code='%2F%2F%20Example%201z32%200x8yyyz8%20255wSAR'~0000000zwPUSHy~~~w%5Cn%01wyz~_

    let mut value: [u8; 32] = [0; 32];
    value[0] = 0b10000000;
    let value = BigUint::from_bytes_be(&value);

    let shift: u8 = 255;
    let program = vec![
        Operation::Push(value),
        Operation::Push(BigUint::from(shift)),
        Operation::Sar,
    ];
    let expected_result = 0b11111111;
    run_program_assert_result(program, expected_result);
}

#[test]
fn sar_with_positive_value_preserves_sign() {
    let mut value: [u8; 32] = [0xff; 32];
    value[0] = 0;
    let value = BigUint::from_bytes_be(&value);

    let shift: u8 = 255;
    let program = vec![
        Operation::Push(value),
        Operation::Push(BigUint::from(shift)),
        Operation::Sar,
    ];
    let expected_result = 0;
    run_program_assert_result(program, expected_result);
}

#[test]
fn sar_with_shift_out_of_bounds() {
    // even if the shift is larger than 255 the SAR operation should
    // work the same.

    let value = BigUint::from_bytes_be(&[0xff; 32]);
    let shift: usize = 1024;
    let program = vec![
        Operation::Push(value),
        Operation::Push(BigUint::from(shift)),
        Operation::Sar,
    ];
    // in this case the expected result is 0xff because of the sign extension
    let expected_result = 0xff;
    run_program_assert_result(program, expected_result);
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
fn jumpdest_gas_should_revert() {
    let program = vec![
        Operation::Push0,
        Operation::Jumpdest { pc: 0 },
        Operation::Jumpdest { pc: 1 },
        Operation::Jumpdest { pc: 2 },
    ];
    let needed_gas = gas_cost::PUSH0 + gas_cost::JUMPDEST * 3;
    run_program_assert_gas_exact(program, 0, needed_gas as _);
}

#[test]
fn test_eq_true() {
    let program = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(BigUint::from(1_u8)),
        Operation::Eq,
    ];
    run_program_assert_result(program, 1);
}

#[test]
fn test_eq_false() {
    let program = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Eq,
    ];
    run_program_assert_result(program, 0);
}

#[test]
fn test_eq_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Eq]);
}

#[test]
fn test_or() {
    let a = BigUint::from(0b1010_u8);
    let b = BigUint::from(0b1110_u8);
    let expected = 0b1110_u8;
    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Or,
    ];
    run_program_assert_result(program, expected);
}

#[test]
fn test_or_with_stack_underflow() {
    let program = vec![Operation::Or];
    run_program_assert_revert(program);
}

#[test]
fn jumpi_with_true_condition() {
    // this test is equivalent to the following bytecode program
    //
    // [00] PUSH1 5
    // [02] PUSH1 1  // push condition
    // [04] PUSH1 9  // push pc
    // [06] JUMPI
    // [07] PUSH1 10
    // [09] JUMPDEST
    let (a, b) = (5_u8, 10_u8);
    let condition: BigUint = BigUint::from(1_u8);
    let pc: usize = 9;
    let program = vec![
        Operation::Push(BigUint::from(a)),
        Operation::Push(condition),
        Operation::Push(BigUint::from(pc as u8)),
        Operation::Jumpi,
        Operation::Push(BigUint::from(b)), // this should not be executed
        Operation::Jumpdest { pc },
    ];
    run_program_assert_result(program, a);
}

#[test]
fn test_iszero_true() {
    let program = vec![Operation::Push(BigUint::from(0_u8)), Operation::IsZero];
    run_program_assert_result(program, 1);
}

#[test]
fn test_iszero_false() {
    let program = vec![Operation::Push(BigUint::from(1_u8)), Operation::IsZero];
    run_program_assert_result(program, 0);
}

#[test]
fn test_iszero_stack_underflow() {
    let program = vec![Operation::IsZero];
    run_program_assert_revert(program);
}

#[test]
fn jump() {
    // this test is equivalent to the following bytecode program
    // the program executes sequentially until the JUMP where
    // it jumps to the opcode in the position 7 so the PUSH1 10
    // opcode is not executed => the return value should be equal
    // to the first pushed value (a = 5)
    //
    // [00] PUSH1 5
    // [02] PUSH1 7  // push pc
    // [04] JUMP
    // [05] PUSH1 10
    // [07] JUMPDEST
    let (a, b) = (5_u8, 10_u8);
    let pc: usize = 7;
    let program = vec![
        Operation::Push(BigUint::from(a)),
        Operation::Push(BigUint::from(pc as u8)),
        Operation::Jump,
        Operation::Push(BigUint::from(b)), // this should not be executed
        Operation::Jumpdest { pc },
    ];
    run_program_assert_result(program, a);
}

#[test]
fn jumpi_with_false_condition() {
    // this test is equivalent to the following bytecode program
    //
    // [00] PUSH1 5
    // [02] PUSH1 0  // push condition
    // [04] PUSH1 9  // push pc
    // [06] JUMPI
    // [07] PUSH1 10
    // [09] JUMPDEST
    let (a, b) = (5_u8, 10_u8);
    let condition: BigUint = BigUint::from(0_u8);
    let pc: usize = 9;
    let program = vec![
        Operation::Push(BigUint::from(a)),
        Operation::Push(condition),
        Operation::Push(BigUint::from(pc as u8)),
        Operation::Jumpi,
        Operation::Push(BigUint::from(b)),
        Operation::Jumpdest { pc },
    ];
    run_program_assert_result(program, b);
}

#[test]
fn jumpi_reverts_if_pc_is_wrong() {
    // if the pc given does not correspond to a jump destination then
    // the program should revert
    let pc = BigUint::from(7_u8);
    let condition = BigUint::from(1_u8);
    let program = vec![
        Operation::Push(condition),
        Operation::Push(pc),
        Operation::Jumpi,
        Operation::Jumpdest { pc: 83 },
    ];
    run_program_assert_revert(program);
}

#[test]
fn jump_reverts_if_pc_is_wrong() {
    // if the pc given does not correspond to a jump destination then
    // the program should revert
    let pc = BigUint::from(7_u8);
    let program = vec![
        Operation::Push(pc),
        Operation::Jump,
        Operation::Jumpdest { pc: 83 },
    ];
    run_program_assert_revert(program);
}

#[test]
fn jumpi_does_not_revert_if_pc_is_wrong_but_branch_is_not_taken() {
    // if the pc given does not correspond to a jump destination
    // but the branch is not taken then the program should not revert
    let pc = BigUint::from(7_u8);
    let condition = BigUint::from(0_u8);
    let a = 10_u8;
    let program = vec![
        Operation::Push(condition),
        Operation::Push(pc),
        Operation::Jumpi,
        Operation::Push(BigUint::from(a)),
        Operation::Jumpdest { pc: 83 },
    ];
    run_program_assert_result(program, a);
}

#[test]
fn pc_with_previous_push() {
    let pc = 33;
    let program = vec![
        Operation::Push(BigUint::from(8_u8)), // <No collapse>
        Operation::PC { pc },                 // <No collapse>
    ];
    run_program_assert_result(program, pc as u8)
}

#[test]
fn pc_with_no_previous_operation() {
    let pc = 0;
    let program = vec![
        Operation::PC { pc }, // <No collapse>
    ];
    run_program_assert_result(program, pc as u8)
}

#[test]
fn pc_gas_should_revert() {
    let program = vec![Operation::Push0, Operation::PC { pc: 0 }];
    let needed_gas = gas_cost::PUSH0 + gas_cost::PC;
    run_program_assert_gas_exact(program, 0, needed_gas as _);
}

#[test]
fn check_initial_memory_size() {
    let program = vec![Operation::Msize];

    run_program_assert_result(program, 0)
}

#[test]
fn msize_out_of_gas() {
    let program = vec![Operation::Msize];
    let gas_needed = gas_cost::MSIZE;

    run_program_assert_gas_exact(program, 0, gas_needed as _);
}

#[test]
fn test_and() {
    let (a, b) = (BigUint::from(0b1010_u8), BigUint::from(0b1100_u8));
    let expected_result = 0b1000_u8;
    let program = vec![Operation::Push(a), Operation::Push(b), Operation::And];
    run_program_assert_result(program, expected_result);
}
#[test]
fn test_and_with_zero() {
    let a = BigUint::from(0_u8);
    let b = BigUint::from(0xFF_u8);
    let expected_result = 0_u8;
    let program = vec![Operation::Push(a), Operation::Push(b), Operation::And];
    run_program_assert_result(program, expected_result);
}

#[test]
fn and_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::And]);
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
fn mod_reverts_when_program_runs_out_of_gas() {
    let (a, b) = (5_u8, 10_u8);
    let program: Vec<Operation> = vec![
        Operation::Push(BigUint::from(b)),
        Operation::Push(BigUint::from(a)),
        Operation::Mod,
    ];
    let initial_gas = gas_cost::PUSHN * 2 + gas_cost::MOD;
    let expected_result = a % b;
    run_program_assert_gas_exact(program, expected_result, initial_gas as _);
}

#[test]
fn smod_with_negative_operands() {
    // -8 mod -3 = -2
    let num = biguint_256_from_bigint(BigInt::from(-8_i8));
    let den = biguint_256_from_bigint(BigInt::from(-3_i8));

    let expected_result = biguint_256_from_bigint(BigInt::from(-2_i8));
    let result_last_byte = expected_result.to_bytes_be()[31];

    let program = vec![Operation::Push(den), Operation::Push(num), Operation::SMod];
    run_program_assert_result(program, result_last_byte);
}

#[test]
fn smod_with_negative_denominator() {
    // 8 mod -3 = 2
    let num = BigUint::from(8_u8);
    let den = biguint_256_from_bigint(BigInt::from(-3_i8));

    let expected_result = BigUint::from(2_u8);

    let program = vec![Operation::Push(den), Operation::Push(num), Operation::SMod];
    run_program_assert_result(program, expected_result.try_into().unwrap());
}

#[test]
fn smod_with_negative_numerator() {
    // -8 mod 3 = -2
    let num = biguint_256_from_bigint(BigInt::from(-8_i8));
    let den = BigUint::from(3_u8);

    let expected_result = biguint_256_from_bigint(BigInt::from(-2_i8));
    let result_last_byte = expected_result.to_bytes_be()[31];

    let program = vec![Operation::Push(den), Operation::Push(num), Operation::SMod];
    run_program_assert_result(program, result_last_byte);
}

#[test]
fn smod_with_positive_operands() {
    let (num, den) = (BigUint::from(31_u8), BigUint::from(10_u8));
    let expected_result = (&num % &den).try_into().unwrap();

    let program = vec![Operation::Push(den), Operation::Push(num), Operation::SMod];
    run_program_assert_result(program, expected_result);
}

#[test]
fn smod_with_zero_denominator() {
    let (num, den) = (BigUint::from(10_u8), BigUint::from(0_u8));

    let program = vec![Operation::Push(den), Operation::Push(num), Operation::SMod];
    run_program_assert_result(program, 0);
}

#[test]
fn smod_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::SMod]);
}

#[test]
fn smod_reverts_when_program_runs_out_of_gas() {
    let (a, b) = (5_u8, 10_u8);
    let program = vec![
        Operation::Push(BigUint::from(b)),
        Operation::Push(BigUint::from(a)),
        Operation::SMod,
    ];
    let expected_result = a % b;
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::SMOD;
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
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
fn addmod_with_overflowing_add() {
    let (a, b, den) = (
        BigUint::from_bytes_be(&[0xff; 32]),
        BigUint::from(1_u8),
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
fn addmod_reverts_when_program_runs_out_of_gas() {
    let (a, b, den) = (
        BigUint::from(5_u8),
        BigUint::from(10_u8),
        BigUint::from(2_u8),
    );

    let program = vec![
        Operation::Push(den.clone()),
        Operation::Push(b.clone()),
        Operation::Push(a.clone()),
        Operation::Addmod,
    ];

    let needed_gas = gas_cost::PUSHN * 3 + gas_cost::ADDMOD;
    let expected_result = ((a + b) % den).try_into().unwrap();

    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn test_gt_less_than() {
    let a = BigUint::from(9_u8);
    let b = BigUint::from(8_u8);
    let program = vec![Operation::Push(a), Operation::Push(b), Operation::Gt];
    run_program_assert_result(program, 1);
}

#[test]
fn test_gt_greater_than() {
    let a = BigUint::from(8_u8);
    let b = BigUint::from(9_u8);
    let program = vec![Operation::Push(a), Operation::Push(b), Operation::Gt];
    run_program_assert_result(program, 0);
}

#[test]
fn test_gt_equal() {
    let a = BigUint::from(10_u8);
    let b = BigUint::from(10_u8);
    let program = vec![Operation::Push(a), Operation::Push(b), Operation::Gt];
    run_program_assert_result(program, 0);
}

#[test]
fn gt_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Gt]);
}

#[test]
fn mulmod_with_non_zero_result() {
    let (a, b, den) = (
        BigUint::from(13_u8),
        BigUint::from(30_u8),
        BigUint::from(10_u8),
    );

    let program = vec![
        Operation::Push(den.clone()),
        Operation::Push(b.clone()),
        Operation::Push(a.clone()),
        Operation::Mulmod,
    ];
    run_program_assert_result(program, ((a * b) % den).try_into().unwrap());
}

#[test]
fn mulmod_with_stack_underflow() {
    run_program_assert_revert(vec![Operation::Mulmod]);
}

#[test]
fn mulmod_with_zero_denominator() {
    let program = vec![
        Operation::Push(BigUint::from(0_u8)),
        Operation::Push(BigUint::from(31_u8)),
        Operation::Push(BigUint::from(11_u8)),
        Operation::Addmod,
    ];
    run_program_assert_result(program, 0);
}

#[test]
fn mulmod_with_overflow() {
    let (a, b, den) = (
        BigUint::from_bytes_be(&[0xff; 32]),
        BigUint::from_bytes_be(&[0xff; 32]),
        BigUint::from(10_u8),
    );

    let program = vec![
        Operation::Push(den.clone()),
        Operation::Push(b.clone()),
        Operation::Push(a.clone()),
        Operation::Mulmod,
    ];
    run_program_assert_result(program, ((a * b) % den).try_into().unwrap());
}

#[test]
fn mulmod_reverts_when_program_runs_out_of_gas() {
    let (a, b, den) = (
        BigUint::from(13_u8),
        BigUint::from(30_u8),
        BigUint::from(10_u8),
    );

    let program = vec![
        Operation::Push(den.clone()),
        Operation::Push(b.clone()),
        Operation::Push(a.clone()),
        Operation::Mulmod,
    ];

    let needed_gas = gas_cost::PUSHN * 3 + gas_cost::MULMOD;
    let expected_result = ((a * b) % den).try_into().unwrap();

    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn test_sgt_positive_greater_than() {
    let a = BigUint::from(2_u8);
    let b = BigUint::from(1_u8);

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Sgt,
    ];
    run_program_assert_result(program, 0);
}

#[test]
fn test_sgt_positive_less_than() {
    let a = BigUint::from(0_u8);
    let b = BigUint::from(2_u8);

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Sgt,
    ];
    run_program_assert_result(program, 1);
}

#[test]
fn test_sgt_signed_less_than() {
    let mut a = BigUint::from(3_u8);
    a.set_bit(255, true);
    let b = BigUint::from(2_u8);

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Sgt,
    ];

    run_program_assert_result(program, 1);
}

#[test]
fn test_sgt_signed_greater_than() {
    let a = BigUint::from(2_u8);
    let mut b = BigUint::from(3_u8);
    b.set_bit(255, true);

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Sgt,
    ];
    run_program_assert_result(program, 0);
}

#[test]
fn test_sgt_equal() {
    let a = BigUint::from(2_u8);
    let b = BigUint::from(2_u8);

    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Sgt,
    ];
    run_program_assert_result(program, 0);
}

#[test]
fn test_sgt_stack_underflow() {
    let program = vec![Operation::Sgt];
    run_program_assert_revert(program);
}

#[test]
fn test_lt_false() {
    let program = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(BigUint::from(2_u8)),
        Operation::Lt,
    ];
    run_program_assert_result(program, 0);
}

#[test]
fn test_lt_true() {
    let program = vec![
        Operation::Push(BigUint::from(2_u8)),
        Operation::Push(BigUint::from(1_u8)),
        Operation::Lt,
    ];
    run_program_assert_result(program, 1);
}

#[test]
fn test_lt_equal() {
    let program = vec![
        Operation::Push(BigUint::from(1_u8)),
        Operation::Push(BigUint::from(1_u8)),
        Operation::Lt,
    ];
    run_program_assert_result(program, 0);
}

#[test]
fn test_lt_stack_underflow() {
    let program = vec![Operation::Lt];
    run_program_assert_revert(program);
}

#[test]
fn test_gas_with_add_should_revert() {
    let x = 1_u8;

    let program = vec![
        Operation::Push(BigUint::from(x)),
        Operation::Push(BigUint::from(x)),
        Operation::Add,
    ];
    let expected_result = x + x;
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::ADD;
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn stop() {
    // the push operation should not be executed
    let program = vec![Operation::Stop, Operation::Push(BigUint::from(10_u8))];
    run_program_assert_result(program, 0);
}

#[test]
fn push_push_exp() {
    let (a, b) = (BigUint::from(2_u8), BigUint::from(3_u8));
    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Exp,
    ];

    run_program_assert_result(program, (a.pow(b.try_into().unwrap())).try_into().unwrap());
}

#[test]
fn exp_with_overflow_should_wrap() {
    let a = BigUint::from(3_u8);
    let b = BigUint::from(256_u16);
    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Exp,
    ];
    run_program_assert_result(program, 1);
}

#[test]
fn exp_with_stack_underflow() {
    let program = vec![Operation::Exp];
    run_program_assert_revert(program);
}

#[test]
fn sar_reverts_when_program_runs_out_of_gas() {
    let (value, shift) = (2_u8, 1_u8);
    let program: Vec<Operation> = vec![
        Operation::Push(BigUint::from(value)),
        Operation::Push(BigUint::from(shift)),
        Operation::Sar,
    ];
    let needed_gas = gas_cost::PUSHN + gas_cost::PUSHN + gas_cost::ADD;
    run_program_assert_gas_exact(program, value >> shift, needed_gas as _);
}

#[test]
fn pop_reverts_when_program_runs_out_of_gas() {
    let expected_result = 33_u8;
    let program = vec![
        Operation::Push(BigUint::from(expected_result)),
        Operation::Push(BigUint::from(expected_result + 1)),
        Operation::Pop,
    ];
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::POP;
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn signextend_one_byte_negative_value() {
    /*
    Since we are constrained by the output size u8, in order to check that the result
    was correctly sign extended (completed with 1s), we have to divide by 2 so we can check
    that the first byte is 0xFF = [1, 1, 1, 1, 1, 1, 1, 1]
    */
    let value = BigUint::from(0xFF_u8);
    let value_bytes_size = BigUint::from(0_u8);
    let denominator = BigUint::from(2_u8);

    let expected_result = 0xFF_u8;

    let program = vec![
        Operation::Push(denominator),      // <No collapse>
        Operation::Push(value),            // <No collapse>
        Operation::Push(value_bytes_size), // <No collapse>
        Operation::SignExtend,             // <No collapse>
        Operation::Div,
    ];
    run_program_assert_result(program, expected_result);
}

#[test]
fn signextend_one_byte_positive_value() {
    /*
    Since we are constrained by the output size u8, in order to check that the result
    was correctly sign extended (completed with 0s), we have to divide by 2 so we can check
    that the first byte is 0x3F = [0, 0, 1, 1, 1, 1, 1, 1]
    */
    let value = BigUint::from(0x7F_u8);
    let value_bytes_size = BigUint::from(0_u8);
    let denominator = BigUint::from(2_u8);

    let expected_result = 0x3F_u8;

    let program = vec![
        Operation::Push(denominator),      // <No collapse>
        Operation::Push(value),            // <No collapse>
        Operation::Push(value_bytes_size), // <No collapse>
        Operation::SignExtend,             // <No collapse>
        Operation::Div,
    ];

    run_program_assert_result(program, expected_result);
}

#[test]
fn signextend_with_stack_underflow() {
    let program = vec![Operation::SignExtend];
    run_program_assert_revert(program);
}

#[test]
fn signextend_gas_should_revert() {
    let value = BigUint::from(0x7F_u8);
    let value_bytes_size = BigUint::from(0_u8);
    let program = vec![
        Operation::Push(value.clone()),
        Operation::Push(value_bytes_size.clone()),
        Operation::SignExtend,
    ];
    let expected_result = value.try_into().unwrap();
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::SIGNEXTEND;
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn gas_get_starting_value() {
    const INITIAL_GAS: i64 = 30;

    let expected_result = (INITIAL_GAS - gas_cost::GAS) as _;

    let program = vec![
        Operation::Gas, // <No collapse>
    ];

    run_program_assert_result_with_gas(program, expected_result, INITIAL_GAS as _);
}

#[test]
fn gas_value_after_operations() {
    const INITIAL_GAS: i64 = 50;

    let gas_consumption = gas_cost::PUSHN * 3 + gas_cost::ADD * 2 + gas_cost::GAS;
    let expected_result = (INITIAL_GAS - gas_consumption) as _;

    let program = vec![
        Operation::Push(BigUint::ZERO), // <No collapse>
        Operation::Push(BigUint::ZERO), // <No collapse>
        Operation::Push(BigUint::ZERO), // <No collapse>
        Operation::Add,                 // <No collapse>
        Operation::Add,                 // <No collapse>
        Operation::Gas,                 // <No collapse>
    ];

    run_program_assert_result_with_gas(program, expected_result, INITIAL_GAS as _);
}

#[test]
fn gas_without_enough_gas_revert() {
    let gas_consumption = gas_cost::PUSHN * 3 + gas_cost::ADD * 2 + gas_cost::GAS;
    let expected_result = 0;

    let program = vec![
        Operation::Push(BigUint::ZERO), // <No collapse>
        Operation::Push(BigUint::ZERO), // <No collapse>
        Operation::Push(BigUint::ZERO), // <No collapse>
        Operation::Add,                 // <No collapse>
        Operation::Add,                 // <No collapse>
        Operation::Gas,                 // <No collapse>
    ];

    run_program_assert_gas_exact(program, expected_result, gas_consumption as _);
}

#[test]
fn byte_gas_cost() {
    let value: [u8; 32] = [0xff; 32];
    let offset = BigUint::from(16_u8);
    let program: Vec<Operation> = vec![
        Operation::Push(BigUint::from_bytes_be(&value)),
        Operation::Push(offset),
        Operation::Byte,
    ];
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::BYTE;
    let expected_result = 0xff;
    run_program_assert_result_with_gas(program, expected_result, needed_gas as _);
}

#[test]
fn and_reverts_when_program_run_out_of_gas() {
    let (a, b) = (BigUint::from(0_u8), BigUint::from(1_u8));
    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::And,
    ];
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::AND;
    let expected_result = (a & b).try_into().unwrap();

    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn exp_reverts_when_program_runs_out_of_gas() {
    let program = vec![
        Operation::Push(BigUint::from(3_u8)),
        Operation::Push(BigUint::from(256_u16)),
        Operation::Exp,
    ];

    let initial_gas = gas_cost::PUSHN * 2 + gas_cost::EXP;
    let expected_result = 1;
    run_program_assert_gas_exact(program, expected_result, initial_gas as _);
}

#[test]
fn lt_reverts_when_program_runs_out_of_gas() {
    let (a, b) = (BigUint::from(0_u8), BigUint::from(1_u8));
    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Lt,
    ];
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::LT;
    let expected_result = if a < b { 0 } else { 1 };
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn sgt_reverts_when_program_runs_out_of_gas() {
    let (a, b) = (BigUint::from(0_u8), BigUint::from(1_u8));
    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Sgt,
    ];
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::SGT;
    let expected_result = if a > b { 0 } else { 1 };
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn gt_reverts_when_program_runs_out_of_gas() {
    let (a, b) = (BigUint::from(0_u8), BigUint::from(1_u8));
    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Gt,
    ];
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::GT;
    let expected_result = if a > b { 1 } else { 0 };
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn eq_reverts_when_program_runs_out_of_gas() {
    let (a, b) = (BigUint::from(0_u8), BigUint::from(1_u8));
    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Eq,
    ];
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::EQ;
    let expected_result = if a == b { 1 } else { 0 };
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn iszero_reverts_when_program_runs_out_of_gas() {
    let a = BigUint::from(0_u8);
    let program = vec![Operation::Push(a.clone()), Operation::IsZero];
    let needed_gas = gas_cost::PUSHN + gas_cost::ISZERO;
    let expected_result = if a == 0_u8.into() { 1 } else { 0 };
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn or_reverts_when_program_runs_out_of_gas() {
    let (a, b) = (BigUint::from(0_u8), BigUint::from(1_u8));
    let program = vec![
        Operation::Push(a.clone()),
        Operation::Push(b.clone()),
        Operation::Or,
    ];
    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::OR;
    let expected_result = (a | b).try_into().unwrap();
    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn slt_positive_less_than() {
    let a = BigInt::from(1_u8);
    let b = BigInt::from(2_u8);

    let expected_result = (a < b) as u8;

    let program = vec![
        Operation::Push(biguint_256_from_bigint(b)),
        Operation::Push(biguint_256_from_bigint(a)),
        Operation::Slt,
    ];

    run_program_assert_result(program, expected_result);
}

#[test]
fn slt_positive_greater_than() {
    let a = BigInt::from(2_u8);
    let b = BigInt::from(1_u8);

    let expected_result = (a < b) as u8;

    let program = vec![
        Operation::Push(biguint_256_from_bigint(b)),
        Operation::Push(biguint_256_from_bigint(a)),
        Operation::Slt,
    ];

    run_program_assert_result(program, expected_result);
}

#[test]
fn slt_negative_less_than() {
    let a = BigInt::from(-3_i8);
    let b = BigInt::from(-1_i8);

    let expected_result = (a < b) as u8;

    let program = vec![
        Operation::Push(biguint_256_from_bigint(b)),
        Operation::Push(biguint_256_from_bigint(a)),
        Operation::Slt,
    ];

    run_program_assert_result(program, expected_result);
}

#[test]
fn slt_negative_greater_than() {
    let a = BigInt::from(0_i8);
    let b = BigInt::from(-1_i8);

    let expected_result = (a < b) as u8;

    let program = vec![
        Operation::Push(biguint_256_from_bigint(b)),
        Operation::Push(biguint_256_from_bigint(a)),
        Operation::Slt,
    ];

    run_program_assert_result(program, expected_result);
}

#[test]
fn slt_equal() {
    let a = BigInt::from(-4_i8);
    let b = BigInt::from(-4_i8);

    let expected_result = (a < b) as u8;

    let program = vec![
        Operation::Push(biguint_256_from_bigint(b)),
        Operation::Push(biguint_256_from_bigint(a)),
        Operation::Slt,
    ];

    run_program_assert_result(program, expected_result);
}

#[test]
fn slt_gas_should_revert() {
    let a = BigInt::from(1_u8);
    let b = BigInt::from(2_u8);

    let expected_result = (a < b) as u8;

    let needed_gas = gas_cost::PUSHN * 2 + gas_cost::SLT;

    let program = vec![
        Operation::Push(biguint_256_from_bigint(b)),
        Operation::Push(biguint_256_from_bigint(a)),
        Operation::Slt,
    ];

    run_program_assert_gas_exact(program, expected_result, needed_gas as _);
}

#[test]
fn slt_stack_underflow() {
    let program = vec![Operation::Slt];
    run_program_assert_revert(program);
}
