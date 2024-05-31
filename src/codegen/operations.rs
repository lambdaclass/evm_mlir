use melior::{
    dialect::{arith, cf},
    ir::{Attribute, Block, BlockRef, Location, Region},
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
        Operation::Push(x) => codegen_push(op_ctx, region, x),
        Operation::Add => codegen_add(op_ctx, region),
        Operation::Mul => codegen_mul(op_ctx, region),
        Operation::Pop => codegen_pop(op_ctx, region),
        Operation::Gt => codegen_gt(op_ctx, region),
        Operation::Jumpdest { pc } => codegen_jumpdest(op_ctx, region, pc),
    }
}

fn codegen_gt<'c, 'r>(
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

    let rhs = stack_pop(context, &ok_block)?;
    let lhs = stack_pop(context, &ok_block)?;

    let result = ok_block
        .append_operation(arith::cmpi(
            context,
            arith::CmpiPredicate::Ugt,
            lhs,
            rhs,
            location,
        ))
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

    Ok((start_block, ok_block))
}

// TODO: use const generics to generalize for pushN
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
