use melior::{
    dialect::{arith, cf, func, ods},
    ir::{
        attribute::IntegerAttribute, r#type::IntegerType, Attribute, Block, BlockRef, Location,
        Region,
    },
};

use super::context::OperationCtx;
use crate::{
    errors::CodegenError,
    program::Operation,
    utils::{
        check_if_zero, check_is_greater_than, check_stack_has_at_least, check_stack_has_space_for,
        consume_gas, extend_memory, get_nth_from_stack, integer_constant_from_i64,
        integer_constant_from_i8, stack_pop, stack_push, swap_stack_elements,
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
        Operation::Stop => codegen_stop(op_ctx, region),
        Operation::Push(x) => codegen_push(op_ctx, region, x),
        Operation::Sgt => codegen_sgt(op_ctx, region),
        Operation::Add => codegen_add(op_ctx, region),
        Operation::Sub => codegen_sub(op_ctx, region),
        Operation::Mul => codegen_mul(op_ctx, region),
        Operation::Xor => codegen_xor(op_ctx, region),
        Operation::Div => codegen_div(op_ctx, region),
        Operation::Shr => codegen_shr(op_ctx, region),
        Operation::Mod => codegen_mod(op_ctx, region),
        Operation::Addmod => codegen_addmod(op_ctx, region),
        Operation::Mulmod => codegen_mulmod(op_ctx, region),
        Operation::Pop => codegen_pop(op_ctx, region),
        Operation::Eq => codegen_eq(op_ctx, region),
        Operation::PC { pc } => codegen_pc(op_ctx, region, pc),
        Operation::Gt => codegen_gt(op_ctx, region),
        Operation::Lt => codegen_lt(op_ctx, region),
        Operation::Jumpdest { pc } => codegen_jumpdest(op_ctx, region, pc),
        Operation::Sar => codegen_sar(op_ctx, region),
        Operation::Dup(x) => codegen_dup(op_ctx, region, x),
        Operation::Swap(x) => codegen_swap(op_ctx, region, x),
        Operation::Byte => codegen_byte(op_ctx, region),
        Operation::Exp => codegen_exp(op_ctx, region),
        Operation::Jumpi => codegen_jumpi(op_ctx, region),
        Operation::IsZero => codegen_iszero(op_ctx, region),
        Operation::Jump => codegen_jump(op_ctx, region),
        Operation::And => codegen_and(op_ctx, region),
        Operation::Or => codegen_or(op_ctx, region),
        Operation::Return => codegen_return(op_ctx, region),
        Operation::Revert => codegen_revert(op_ctx, region),
    }
}

fn codegen_exp<'c, 'r>(
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
        .append_operation(ods::math::ipowi(context, rhs, lhs, location).into())
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

    Ok((start_block, ok_block))
}

fn codegen_iszero<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
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

    let value = stack_pop(context, &ok_block)?;
    let value_is_zero = check_if_zero(context, &ok_block, &value)?;

    let val_zero_bloq = region.append_block(Block::new(&[]));
    let val_not_zero_bloq = region.append_block(Block::new(&[]));
    let return_block = region.append_block(Block::new(&[]));

    let constant_value = val_zero_bloq
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, 1i64).into(),
            location,
        ))
        .result(0)?
        .into();

    stack_push(context, &val_zero_bloq, constant_value)?;
    val_zero_bloq.append_operation(cf::br(&return_block, &[], location));

    let result = val_not_zero_bloq
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, 0i64).into(),
            location,
        ))
        .result(0)?
        .into();

    stack_push(context, &val_not_zero_bloq, result)?;
    val_not_zero_bloq.append_operation(cf::br(&return_block, &[], location));

    ok_block.append_operation(cf::cond_br(
        context,
        value_is_zero,
        &val_zero_bloq,
        &val_not_zero_bloq,
        &[],
        &[],
        location,
    ));

    Ok((start_block, return_block))
}

fn codegen_and<'c, 'r>(
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
        .append_operation(arith::andi(lhs, rhs, location))
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

    Ok((start_block, ok_block))
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

fn codegen_or<'c, 'r>(
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
        .append_operation(arith::ori(lhs, rhs, location))
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

    Ok((start_block, ok_block))
}

fn codegen_lt<'c, 'r>(
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
        .append_operation(arith::cmpi(
            context,
            arith::CmpiPredicate::Ult,
            lhs,
            rhs,
            location,
        ))
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

    Ok((start_block, ok_block))
}

fn codegen_sgt<'c, 'r>(
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
        .append_operation(arith::cmpi(
            context,
            arith::CmpiPredicate::Sgt,
            lhs,
            rhs,
            location,
        ))
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

    Ok((start_block, ok_block))
}

fn codegen_eq<'c, 'r>(
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
        .append_operation(arith::cmpi(
            context,
            arith::CmpiPredicate::Eq,
            lhs,
            rhs,
            location,
        ))
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

    Ok((start_block, ok_block))
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

fn codegen_dup<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
    nth: u32,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    debug_assert!(nth > 0 && nth <= 16);
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
    let flag = check_stack_has_at_least(context, &start_block, nth)?;

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

    let (nth_value, _) = get_nth_from_stack(context, &ok_block, nth)?;

    stack_push(context, &ok_block, nth_value)?;

    Ok((start_block, ok_block))
}

fn codegen_swap<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
    nth: u32,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    debug_assert!(nth > 0 && nth <= 16);
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
    let flag = check_stack_has_at_least(context, &start_block, nth + 1)?;

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

    swap_stack_elements(context, &ok_block, 1, nth + 1)?;

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

    let gas_flag = consume_gas(context, &start_block, 3)?;

    let condition = start_block
        .append_operation(arith::andi(gas_flag, flag, location))
        .result(0)?
        .into();

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        condition,
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

fn codegen_sub<'c, 'r>(
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
        .append_operation(arith::subi(lhs, rhs, location))
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

    let den_is_zero = check_if_zero(context, &ok_block, &den)?;
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

fn codegen_mod<'c, 'r>(
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

    let den_is_zero = check_if_zero(context, &ok_block, &den)?;
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

    let mod_result = den_not_zero_bloq
        .append_operation(arith::remui(num, den, location))
        .result(0)?
        .into();

    stack_push(context, &den_not_zero_bloq, mod_result)?;

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

fn codegen_addmod<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
    let flag = check_stack_has_at_least(context, &start_block, 3)?;

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

    let a = stack_pop(context, &ok_block)?;
    let b = stack_pop(context, &ok_block)?;
    let den = stack_pop(context, &ok_block)?;

    let den_is_zero = check_if_zero(context, &ok_block, &den)?;
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
    let uint256 = IntegerType::new(context, 256).into();
    let uint257 = IntegerType::new(context, 257).into();

    // extend the operands to 257 bits before the addition
    let extended_a = den_not_zero_bloq
        .append_operation(arith::extui(a, uint257, location))
        .result(0)?
        .into();
    let extended_b = den_not_zero_bloq
        .append_operation(arith::extui(b, uint257, location))
        .result(0)?
        .into();
    let extended_den = den_not_zero_bloq
        .append_operation(arith::extui(den, uint257, location))
        .result(0)?
        .into();
    let add_result = den_not_zero_bloq
        .append_operation(arith::addi(extended_a, extended_b, location))
        .result(0)?
        .into();
    let mod_result = den_not_zero_bloq
        .append_operation(arith::remui(add_result, extended_den, location))
        .result(0)?
        .into();
    let truncated_result = den_not_zero_bloq
        .append_operation(arith::trunci(mod_result, uint256, location))
        .result(0)?
        .into();

    stack_push(context, &den_not_zero_bloq, truncated_result)?;

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

fn codegen_mulmod<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
    let flag = check_stack_has_at_least(context, &start_block, 3)?;

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

    let a = stack_pop(context, &ok_block)?;
    let b = stack_pop(context, &ok_block)?;
    let den = stack_pop(context, &ok_block)?;

    let den_is_zero = check_if_zero(context, &ok_block, &den)?;
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

    let uint256 = IntegerType::new(context, 256).into();
    let uint512 = IntegerType::new(context, 512).into();

    // extend the operands to 512 bits before the multiplication
    let extended_a = den_not_zero_bloq
        .append_operation(arith::extui(a, uint512, location))
        .result(0)?
        .into();
    let extended_b = den_not_zero_bloq
        .append_operation(arith::extui(b, uint512, location))
        .result(0)?
        .into();
    let extended_den = den_not_zero_bloq
        .append_operation(arith::extui(den, uint512, location))
        .result(0)?
        .into();

    let mul_result = den_not_zero_bloq
        .append_operation(arith::muli(extended_a, extended_b, location))
        .result(0)?
        .into();
    let mod_result = den_not_zero_bloq
        .append_operation(arith::remui(mul_result, extended_den, location))
        .result(0)?
        .into();
    let truncated_result = den_not_zero_bloq
        .append_operation(arith::trunci(mod_result, uint256, location))
        .result(0)?
        .into();

    stack_push(context, &den_not_zero_bloq, truncated_result)?;
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

fn codegen_xor<'c, 'r>(
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
        .append_operation(arith::xori(lhs, rhs, location))
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

    Ok((start_block, ok_block))
}

fn codegen_shr<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);
    let uint256 = IntegerType::new(context, 256);

    // Check there's enough elements in stack
    let mut flag = check_stack_has_at_least(context, &start_block, 2)?;

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

    let shift = stack_pop(context, &ok_block)?;
    let value = stack_pop(context, &ok_block)?;

    let value_255 = ok_block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint256.into(), 255_i64).into(),
            location,
        ))
        .result(0)?
        .into();

    flag = check_is_greater_than(context, &ok_block, shift, value_255)?;

    let ok_ok_block = region.append_block(Block::new(&[]));
    let altv_block = region.append_block(Block::new(&[]));
    // to unify the blocks after the branching
    let empty_block = region.append_block(Block::new(&[]));

    ok_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_ok_block,
        &altv_block,
        &[],
        &[],
        location,
    ));

    // if shift is less than 255
    let result = ok_ok_block
        .append_operation(arith::shrui(value, shift, location))
        .result(0)?
        .into();

    stack_push(context, &ok_ok_block, result)?;

    ok_ok_block.append_operation(cf::br(&empty_block, &[], location));

    // if shifht is grater than 255
    let result = altv_block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint256.into(), 0_i64).into(),
            location,
        ))
        .result(0)?
        .into();

    stack_push(context, &altv_block, result)?;

    altv_block.append_operation(cf::br(&empty_block, &[], location));

    Ok((start_block, empty_block))
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

    let gas_flag = consume_gas(context, &start_block, 2)?;

    let condition = start_block
        .append_operation(arith::andi(gas_flag, flag, location))
        .result(0)?
        .into();

    let ok_block = region.append_block(Block::new(&[]));

    start_block.append_operation(cf::cond_br(
        context,
        condition,
        &ok_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    stack_pop(context, &ok_block)?;

    Ok((start_block, ok_block))
}

fn codegen_sar<'c, 'r>(
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

    let shift = stack_pop(context, &ok_block)?;
    let value = stack_pop(context, &ok_block)?;

    // max_shift = 255
    let max_shift = ok_block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, 255).into(),
            location,
        ))
        .result(0)?
        .into();

    // if shift > 255  then after applying the `shrsi` operation the result will be poisoned
    // to avoid the poisoning we set shift = min(shift, 255)
    let shift = ok_block
        .append_operation(arith::minui(shift, max_shift, location))
        .result(0)?
        .into();

    let result = ok_block
        .append_operation(arith::shrsi(value, shift, location))
        .result(0)?
        .into();

    stack_push(context, &ok_block, result)?;

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

    const BITS_PER_BYTE: i64 = 8;
    const MAX_SHIFT: i64 = 31;

    let constant_bits_per_byte = ok_block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, BITS_PER_BYTE).into(),
            location,
        ))
        .result(0)?
        .into();

    let constant_max_shift_in_bits = ok_block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, MAX_SHIFT * BITS_PER_BYTE).into(),
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
            integer_constant_from_i64(context, 0).into(),
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

    let mask = offset_ok_block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, 0xff).into(),
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

fn codegen_jumpdest<'c>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'c Region<'c>,
    pc: usize,
) -> Result<(BlockRef<'c, 'c>, BlockRef<'c, 'c>), CodegenError> {
    let landing_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough gas to compute the operation
    let gas_flag = consume_gas(context, &landing_block, 1)?;

    let ok_block = region.append_block(Block::new(&[]));

    landing_block.append_operation(cf::cond_br(
        context,
        gas_flag,
        &ok_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    // Register jumpdest block in context
    op_ctx.register_jump_destination(pc, landing_block);

    Ok((landing_block, ok_block))
}

fn codegen_jumpi<'c, 'r: 'c>(
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

    let pc = stack_pop(context, &ok_block)?;
    let condition = stack_pop(context, &ok_block)?;

    let false_block = region.append_block(Block::new(&[]));

    let zero = ok_block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, 0i64).into(),
            location,
        ))
        .result(0)?
        .into();

    // compare  condition > 0  to convert condition from u256 to 1-bit signless integer
    // TODO: change this maybe using arith::trunci
    let condition = ok_block
        .append_operation(arith::cmpi(
            context,
            arith::CmpiPredicate::Ne,
            condition,
            zero,
            location,
        ))
        .result(0)?;

    ok_block.append_operation(cf::cond_br(
        context,
        condition.into(),
        &op_ctx.jumptable_block,
        &false_block,
        &[pc],
        &[],
        location,
    ));

    Ok((start_block, false_block))
}

fn codegen_jump<'c, 'r: 'c>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    // it reverts if Counter offset is not a JUMPDEST.
    // The error is generated even if the JUMP would not have been done

    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    // Check there's enough elements in stack
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

    let pc = stack_pop(context, &ok_block)?;

    // appends operation to ok_block to jump to the `jump table block``
    // in the jump table block the pc is checked and if its ok
    // then it jumps to the block associated with that pc
    op_ctx.add_jump_op(ok_block, pc, location);

    // TODO: we are creating an empty block that won't ever be reached
    // probably there's a better way to do this
    let empty_block = region.append_block(Block::new(&[]));
    Ok((start_block, empty_block))
}

fn codegen_pc<'c>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'c Region<'c>,
    pc: usize,
) -> Result<(BlockRef<'c, 'c>, BlockRef<'c, 'c>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

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

    let pc_value = ok_block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, pc as i64).into(),
            location,
        ))
        .result(0)?
        .into();

    stack_push(context, &ok_block, pc_value)?;

    Ok((start_block, ok_block))
}

fn codegen_return<'c>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'c Region<'c>,
) -> Result<(BlockRef<'c, 'c>, BlockRef<'c, 'c>), CodegenError> {
    // TODO: compute gas cost for memory expansion
    let context = op_ctx.mlir_context;
    let location = Location::unknown(context);

    let uint32 = IntegerType::new(context, 32);

    let start_block = region.append_block(Block::new(&[]));
    let ok_block = region.append_block(Block::new(&[]));

    let flag = check_stack_has_at_least(context, &start_block, 2)?;

    start_block.append_operation(cf::cond_br(
        context,
        flag,
        &ok_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    let offset_u256 = stack_pop(context, &ok_block)?;
    let size_u256 = stack_pop(context, &ok_block)?;

    // NOTE: for simplicity, we're truncating both offset and size to 32 bits here.
    // If any of them were bigger than a u32, we would have ran out of gas before here.
    let offset = ok_block
        .append_operation(arith::trunci(offset_u256, uint32.into(), location))
        .result(0)
        .unwrap()
        .into();

    let size = ok_block
        .append_operation(arith::trunci(size_u256, uint32.into(), location))
        .result(0)
        .unwrap()
        .into();

    let required_size = ok_block
        .append_operation(arith::addi(offset, size, location))
        .result(0)?
        .into();

    extend_memory(op_ctx, &ok_block, required_size)?;

    op_ctx.write_result_syscall(&ok_block, offset, size, location);

    Ok((start_block, ok_block))
}

fn codegen_stop<'c, 'r>(
    op_ctx: &mut OperationCtx<'c>,
    region: &'r Region<'c>,
) -> Result<(BlockRef<'c, 'r>, BlockRef<'c, 'r>), CodegenError> {
    let start_block = region.append_block(Block::new(&[]));
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);

    let zero = start_block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i8(context, 0).into(),
            location,
        ))
        .result(0)?
        .into();

    start_block.append_operation(func::r#return(&[zero], location));
    let empty_block = region.append_block(Block::new(&[]));

    Ok((start_block, empty_block))
}
