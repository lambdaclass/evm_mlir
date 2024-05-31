use melior::{
    dialect::{arith, cf},
    ir::{Attribute, Block, BlockRef, Location, Region},
    Context as MeliorContext,
};

use super::context::OperationCtx;
use crate::{
    errors::CodegenError,
    program::Operation,
    utils::{
        check_denominator_is_zero, check_stack_has_at_least, check_stack_has_space_for,
        integer_constant_from_i64, stack_pop, stack_push,
    },
};
use num_bigint::BigUint;

/// Generates blocks for target [`Operation`].
/// Returns both the starting block, and the unterminated last block of the generated code.
pub fn generate_code_for_op<'c>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'c Region<'c>,
    op: Operation,
) -> Result<(BlockRef<'c, 'c>, BlockRef<'c, 'c>), CodegenError> {
    match op {
        Operation::Add => codegen_add(op_ctx, region),
        Operation::Mul => codegen_mul(op_ctx, region),
        Operation::Div => codegen_div(op_ctx, region),
        Operation::Sdiv => codegen_sdiv(op_ctx, region),
        Operation::Pop => codegen_pop(op_ctx, region),
        Operation::Jumpdest { pc } => codegen_jumpdest(op_ctx, region, pc),
        Operation::Push(x) => codegen_push(op_ctx, region, x),
        Operation::Byte => codegen_byte(op_ctx, region),
    }
}

fn codegen_push<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
    value_to_push: BigUint,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough space in stack
    let flag = check_stack_has_space_for(context, &start_block, 1)?;

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    let constant_value = Attribute::parse(context, &format!("{} : i256", value_to_push)).unwrap();
    let constant_value = ok_block
        .append_operation(arith::constant(context, constant_value, location))
        .result(0)?
        .into();

    stack_push(context, &ok_block, constant_value)?;

    Ok((start_block, ok_block))
}

fn codegen_add<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
    let flag = check_stack_has_at_least(context, &start_block, 2)?;

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    let lhs = stack_pop(context, &ok_block)?;
    let rhs = stack_pop(context, &ok_block)?;

    let result = ok_block
        .append_operation(arith::addi(lhs, rhs, location))
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

    Ok((start_block, ok_block))
}

fn codegen_div<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
    let flag = check_stack_has_at_least(context, &start_block, 2)?;

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    let num = stack_pop(context, &ok_block)?;
    let den = stack_pop(context, &ok_block)?;

    let den_is_zero = check_denominator_is_zero(context, &ok_block, &den)?;
    let den_zero_bloq = region.append_block(Block::new(&[]));
    let den_not_zero_bloq = region.append_block(Block::new(&[]));
    let return_block = region.append_block(Block::new(&[]));

    let constant_value = den_zero_bloq
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, 0i64).into(),
            location,
        ))
        .result(0)?
        .into();

    stack_push(context, &den_zero_bloq, constant_value)?;

    den_zero_bloq.append_operation(cf::br(&return_block, &[], location));

    // Denominator is not zero path
    let result = den_not_zero_bloq
        .append_operation(arith::divui(num, den, location))
        .result(0)?
        .into();

    stack_push(context, &den_not_zero_bloq, result)?;

    den_not_zero_bloq.append_operation(cf::br(&return_block, &[], location));

    ok_block.append_operation(cf::cond_br(
        context,
        den_is_zero,
        &den_zero_bloq,
        &den_not_zero_bloq,
        &[],
        &[],
        location,
    ));

    Ok((start_block, return_block))
}

fn codegen_sdiv<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);
    // Check there's enough elements in stack
    let flag = check_stack_has_at_least(context, &start_block, 2)?;
    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    let num = stack_pop(context, &ok_block)?;
    let den = stack_pop(context, &ok_block)?;
    let den_is_zero = check_denominator_is_zero(context, &ok_block, &den)?;
    let den_zero_bloq = region.append_block(Block::new(&[]));
    let den_not_zero_bloq = region.append_block(Block::new(&[]));
    let return_block = region.append_block(Block::new(&[]));

    // Denominator is zero path
    let constant_value = den_zero_bloq
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, 0i64).into(),
            location,
        ))
        .result(0)?
        .into();

    stack_push(context, &den_zero_bloq, constant_value)?;

    den_zero_bloq.append_operation(cf::br(&return_block, &[], location));

    // Denominator is not zero path
    let result = den_not_zero_bloq
        .append_operation(arith::divsi(num, den, location))
        .result(0)?
        .into();

    stack_push(context, &den_not_zero_bloq, result)?;

    den_not_zero_bloq.append_operation(cf::br(&return_block, &[], location));

    // Branch to den_zero if den_is_zero == true; else branch to den_not_zero
    ok_block.append_operation(cf::cond_br(
        context,
        den_is_zero,
        &den_zero_bloq,
        &den_not_zero_bloq,
        &[],
        &[],
        location,
    ));

    Ok((start_block, return_block))
}

fn codegen_mul<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
    let flag = check_stack_has_at_least(context, &start_block, 2)?;

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    let lhs = stack_pop(context, &ok_block)?;
    let rhs = stack_pop(context, &ok_block)?;

    let result = ok_block
        .append_operation(arith::muli(lhs, rhs, location))
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

    Ok((start_block, ok_block))
}

fn codegen_pop<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's at least 1 element in stack
    let flag = check_stack_has_at_least(context, &start_block, 1)?;

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    stack_pop(context, &ok_block)?;

    Ok((start_block, ok_block))
}

fn codegen_byte<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
    let flag = check_stack_has_at_least(context, &start_block, 2)?;

    let ok_block = region.append_block(Block::new(&[]));

    // in out_of_bounds_block a 0 is pushed to the stack
    let out_of_bounds_block = region.append_block(Block::new(&[]));

    // in offset_ok_block the byte operation is performed
    let offset_ok_block = region.append_block(Block::new(&[]));

    let end_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    let offset = stack_pop(context, &ok_block)?;
    let value = stack_pop(context, &ok_block)?;

    const BITS_PER_BYTE: u8 = 8;
    const MAX_SHIFT: u8 = 31;
    let mut bits_per_byte: [u8; 32] = [0; 32];
    bits_per_byte[31] = BITS_PER_BYTE;

    let mut max_shift_in_bits: [u8; 32] = [0; 32];
    max_shift_in_bits[31] = MAX_SHIFT * BITS_PER_BYTE;

    let constant_bits_per_byte = ok_block
        .append_operation(arith::constant(
            context,
            integer_constant(context, bits_per_byte),
            location,
        ))
        .result(0)?
        .into();

    let constant_max_shift_in_bits = ok_block
        .append_operation(arith::constant(
            context,
            integer_constant(context, max_shift_in_bits),
            location,
        ))
        .result(0)?
        .into();

    let offset_in_bits = ok_block
        .append_operation(arith::muli(offset, constant_bits_per_byte, location))
        .result(0)?
        .into();

    // compare  offset > max_shift?
    let is_offset_out_of_bounds = ok_block
        .append_operation(arith::cmpi(
            context,
            arith::CmpiPredicate::Ugt,
            offset_in_bits,
            constant_max_shift_in_bits,
            location,
        ))
        .result(0)?
        .into();

    // if offset > max_shift => branch to out_of_bounds_block
    // else => branch to offset_ok_block
    ok_block.append_operation(cf::cond_br(
        context,
        is_offset_out_of_bounds,
        &out_of_bounds_block,
        &offset_ok_block,
        &[],
        &[],
        location,
    ));

    let zero = out_of_bounds_block
        .append_operation(arith::constant(
            context,
            integer_constant(context, [0; 32]),
            location,
        ))
        .result(0)?
        .into();

    // push zero to the stack
    stack_push(context, &out_of_bounds_block, zero)?;

    out_of_bounds_block.append_operation(cf::br(&end_block, &[], location));

    // the idea is to use a right shift to place the byte in the right-most side
    // and then apply a bitwise AND with a 0xFF mask
    //
    // for example, if we want to extract the 0xFF byte in the following value
    // (for simplicity the value has fewer bytes than it has in reality)
    //
    // value = 0xAABBCCDDFFAABBCC
    //                   ^^
    //              desired byte
    //
    // we can shift the value to the right
    //
    // value = 0xAABBCCDDFFAABBCC -> 0x000000AABBCCDDFF
    //                   ^^                          ^^
    // and then apply the bitwise AND it to the right to remove the right-side bytes
    //
    //  value = 0x000000AABBCCDDFF
    //          AND
    //  mask  = 0x00000000000000FF
    //------------------------------
    // result = 0x00000000000000FF

    // compute how many bits the value has to be shifted
    // shift_right_in_bits = max_shift - offset
    let shift_right_in_bits = offset_ok_block
        .append_operation(arith::subi(
            constant_max_shift_in_bits,
            offset_in_bits,
            location,
        ))
        .result(0)?
        .into();

    // shift the value to the right
    let shifted_right_value = offset_ok_block
        .append_operation(arith::shrui(value, shift_right_in_bits, location))
        .result(0)?
        .into();

    let mut mask: [u8; 32] = [0; 32];
    mask[31] = 0xff;

    let mask = offset_ok_block
        .append_operation(arith::constant(
            context,
            integer_constant(context, mask),
            location,
        ))
        .result(0)?
        .into();

    // compute (value AND mask)
    let result = offset_ok_block
        .append_operation(arith::andi(shifted_right_value, mask, location))
        .result(0)?
        .into();

    stack_push(context, &offset_ok_block, result)?;

    offset_ok_block.append_operation(cf::br(&end_block, &[], location));

    Ok((start_block, end_block))
}

fn integer_constant(context: &MeliorContext, value: [u8; 32]) -> Attribute {
    let str_value = BigUint::from_bytes_be(&value).to_string();
    // TODO: should we handle this error?
    Attribute::parse(context, &format!("{str_value} : i256")).unwrap()
}

fn codegen_jumpdest<'c>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'c Region<'c>,
    pc: usize,
) -> Result<(BlockRef<'c, 'c>, BlockRef<'c, 'c>), CodegenError> {
    let landing_block = region.append_block(Block::new(&[]));

    // Register jumpdest block in context
    op_ctx.register_jump_destination(pc, landing_block);

    Ok((landing_block, landing_block))
}
