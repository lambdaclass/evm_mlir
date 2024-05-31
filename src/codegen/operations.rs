use melior::{
    dialect::{arith, cf},
    ir::{Attribute, Block, BlockRef, Location, Region},
    Context as MeliorContext,
};
use num_bigint::BigUint;

use super::context::OperationCtx;
use crate::{
    errors::CodegenError,
    program::Operation,
    utils::{
        check_stack_has_at_least, check_stack_has_space_for, generate_revert_block, stack_pop,
        stack_push,
    },
};

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

    // Create REVERT block
    let revert_block = region.append_block(generate_revert_block(context)?);

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &revert_block,
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

    // Create REVERT block
    let revert_block = region.append_block(generate_revert_block(context)?);

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &revert_block,
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

fn codegen_mul<'c, 'r>(
    codegen_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &codegen_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
    let flag = check_stack_has_at_least(context, &start_block, 2)?;

    // Create REVERT block
    let revert_block = region.append_block(generate_revert_block(context)?);

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &revert_block,
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
    codegen_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &codegen_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's at least 1 element in stack
    let flag = check_stack_has_at_least(context, &start_block, 1)?;

    // Create REVERT block
    let revert_block = region.append_block(generate_revert_block(context)?);

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &revert_block,
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

    // Create REVERT block
    let revert_block = region.append_block(generate_revert_block(context)?);

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
        &revert_block,
        &[],
        &[],
        location,
    ));

    let offset = stack_pop(context, &ok_block)?;
    let value = stack_pop(context, &ok_block)?;

    // define the relevant constants
    const BITS_PER_BYTE: u8 = 8;
    const MAX_SHIFT: u8 = 31;
    let mut bits_per_byte: [u8; 32] = [0; 32];
    bits_per_byte[31] = BITS_PER_BYTE;

    let mut max_shift: [u8; 32] = [0; 32];
    max_shift[31] = MAX_SHIFT * BITS_PER_BYTE;
    let mut max_shift_in_bytes: [u8; 32] = [0; 32];
    max_shift_in_bytes[31] = MAX_SHIFT;

    let constant_bits_per_byte = ok_block
        .append_operation(arith::constant(
            context,
            integer_constant(context, bits_per_byte),
            location,
        ))
        .result(0)?
        .into();

    let constant_max_shift = ok_block
        .append_operation(arith::constant(
            context,
            integer_constant(context, max_shift),
            location,
        ))
        .result(0)?
        .into();

    let constant_max_shift_in_bytes = ok_block
        .append_operation(arith::constant(
            context,
            integer_constant(context, max_shift_in_bytes),
            location,
        ))
        .result(0)?
        .into();

    // compare  offset > max_shift?
    let is_offset_out_of_bounds = ok_block
        .append_operation(arith::cmpi(
            context,
            arith::CmpiPredicate::Ugt,
            offset,
            constant_max_shift_in_bytes,
            location,
        ))
        .result(0)?
        .into();

    // if offset > max_shift => branch to out_of_bounds_block
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

    // the idea is to use left and right shifts in order to extract the
    // desired byte from the value, removing the rest of the bytes
    //
    // for example, if we want to extract the 0xFF byte in the following value
    // (for simplicity the value has fewer bytes than it has in reality)
    //
    // value = 0xAABBCCDDFFAABBCC
    //                   ^^
    //              desired byte
    //
    // we can shift the value to the left to remove the left-side bytes that
    // we don't care about
    //
    // value = 0xAABBCCDDFFAABBCC -> 0xFFAABBCC00000000
    //                   ^^            ^^
    // and then shift it to the right to remove the right-side bytes
    //
    // value = 0xFFAABBCC00000000 -> 0x00000000000000FF
    //           ^^                                  ^^

    // in case the offset is ok, compute how many bits the value must be shifted to the left
    // shift_left = offset * bits_per_byte = offset * 8
    let shift_left = offset_ok_block
        .append_operation(arith::muli(offset, constant_bits_per_byte, location))
        .result(0)?
        .into();

    let shifted_left_value = offset_ok_block
        .append_operation(arith::shli(value, shift_left, location))
        .result(0)?
        .into();

    let result = offset_ok_block
        .append_operation(arith::shrui(
            shifted_left_value,
            constant_max_shift,
            location,
        ))
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
