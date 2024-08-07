use bytes::{BufMut, Bytes};
use melior::{
    dialect::{
        arith::{self, CmpiPredicate},
        cf, func,
        llvm::{self, r#type::pointer, AllocaOptions, LoadStoreOptions},
        ods,
    },
    ir::{
        attribute::{DenseI32ArrayAttribute, IntegerAttribute, TypeAttribute},
        operation::OperationResult,
        r#type::IntegerType,
        Block, Location, Region, Value, ValueLike,
    },
    Context as MeliorContext,
};
use sha3::{Digest, Keccak256};

use crate::{
    codegen::context::OperationCtx,
    constants::{
        gas_cost, CALLDATA_PTR_GLOBAL, CALLDATA_SIZE_GLOBAL, GAS_COUNTER_GLOBAL, MAX_STACK_SIZE,
        MEMORY_PTR_GLOBAL, MEMORY_SIZE_GLOBAL, STACK_BASEPTR_GLOBAL, STACK_PTR_GLOBAL,
    },
    errors::CodegenError,
    primitives::{Address, H160, U256},
    syscall::{symbols::CONTEXT_IS_STATIC, ExitStatusCode},
};

// NOTE: the value is of type i64
pub fn get_remaining_gas<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);

    // Get address of gas counter global
    let gas_counter_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            GAS_COUNTER_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load gas counter
    let gas_counter = block
        .append_operation(llvm::load(
            context,
            gas_counter_ptr.into(),
            IntegerType::new(context, 64).into(),
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    Ok(gas_counter)
}

/// Returns true if there is enough Gas
pub fn consume_gas<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    amount: i64,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let uint64 = IntegerType::new(context, 64).into();

    // Get address of gas counter global
    let gas_counter_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            GAS_COUNTER_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load gas counter
    let gas_counter = block
        .append_operation(llvm::load(
            context,
            gas_counter_ptr.into(),
            uint64,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    let gas_value = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint64, amount).into(),
            location,
        ))
        .result(0)?
        .into();

    // Check that gas_counter >= gas_value
    let flag = block
        .append_operation(arith::cmpi(
            context,
            arith::CmpiPredicate::Sge,
            gas_counter,
            gas_value,
            location,
        ))
        .result(0)?;

    // Subtract gas from gas counter
    let new_gas_counter = block
        .append_operation(arith::subi(gas_counter, gas_value, location))
        .result(0)?;

    // Store new gas counter
    let _res = block.append_operation(llvm::store(
        context,
        new_gas_counter.into(),
        gas_counter_ptr.into(),
        location,
        LoadStoreOptions::default(),
    ));

    Ok(flag.into())
}

pub(crate) fn check_context_is_not_static<'c>(
    op_ctx: &'c OperationCtx,
    block: &'c Block,
) -> Result<Value<'c, 'c>, CodegenError> {
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);
    let uint1 = IntegerType::new(context, 1);

    let is_static = context_is_static(op_ctx, block)?;
    let true_value = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint1.into(), 1).into(),
            location,
        ))
        .result(0)?
        .into();

    let is_not_static = block
        .append_operation(arith::xori(is_static, true_value, location))
        .result(0)?
        .into();

    Ok(is_not_static)
}

pub(crate) fn context_is_static<'c>(
    op_ctx: &'c OperationCtx,
    block: &'c Block,
) -> Result<Value<'c, 'c>, CodegenError> {
    let context = &op_ctx.mlir_context;
    let location = Location::unknown(context);
    let uint1 = IntegerType::new(context, 1);
    let ptr_type = pointer(context, 0);
    let static_flag_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            CONTEXT_IS_STATIC,
            ptr_type,
            location,
        ))
        .result(0)?
        .into();
    let is_static = block
        .append_operation(llvm::load(
            context,
            static_flag_ptr,
            uint1.into(),
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    Ok(is_static)
}

pub fn get_stack_pointer<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);

    // Get address of stack pointer global
    let stack_ptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load stack pointer
    let stack_ptr = block
        .append_operation(llvm::load(
            context,
            stack_ptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    Ok(stack_ptr)
}

pub fn inc_stack_pointer<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
) -> Result<(), CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);

    // Get address of stack pointer global
    let stack_ptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load stack pointer
    let stack_ptr = block
        .append_operation(llvm::load(
            context,
            stack_ptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?;

    let uint256 = IntegerType::new(context, 256);
    // Increment stack pointer
    let new_stack_ptr = block
        .append_operation(llvm::get_element_ptr(
            context,
            stack_ptr.into(),
            DenseI32ArrayAttribute::new(context, &[1]),
            uint256.into(),
            ptr_type,
            location,
        ))
        .result(0)?;

    // Store incremented stack pointer
    let res = block.append_operation(llvm::store(
        context,
        new_stack_ptr.into(),
        stack_ptr_ptr.into(),
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    Ok(())
}

/// Returns true if there is enough Gas
pub fn consume_gas_as_value<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    gas_value: Value<'ctx, 'ctx>,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let uint64 = IntegerType::new(context, 64).into();

    // Get address of gas counter global
    let gas_counter_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            GAS_COUNTER_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load gas counter
    let gas_counter = block
        .append_operation(llvm::load(
            context,
            gas_counter_ptr.into(),
            uint64,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    // Check that gas_counter >= gas_value
    let flag = block
        .append_operation(arith::cmpi(
            context,
            arith::CmpiPredicate::Sge,
            gas_counter,
            gas_value,
            location,
        ))
        .result(0)?;

    // Subtract gas from gas counter
    let new_gas_counter = block
        .append_operation(arith::subi(gas_counter, gas_value, location))
        .result(0)?;

    // Store new gas counter
    let _res = block.append_operation(llvm::store(
        context,
        new_gas_counter.into(),
        gas_counter_ptr.into(),
        location,
        LoadStoreOptions::default(),
    ));

    Ok(flag.into())
}

// computes dynamic_gas = 375 * topic_count + 8 * size
pub(crate) fn compute_log_dynamic_gas<'a>(
    op_ctx: &'a OperationCtx<'a>,
    block: &'a Block<'a>,
    nth: u8,
    size: Value<'a, 'a>,
    location: Location<'a>,
) -> Result<Value<'a, 'a>, CodegenError> {
    let context = op_ctx.mlir_context;
    let uint64 = IntegerType::new(context, 64);

    let constant_375 = block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, 375).into(),
            location,
        ))
        .result(0)?
        .into();

    let constant_8 = block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, 8).into(),
            location,
        ))
        .result(0)?
        .into();

    let topic_count = block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, nth as i64).into(),
            location,
        ))
        .result(0)?
        .into();

    let topic_count_x_375 = block
        .append_operation(arith::muli(topic_count, constant_375, location))
        .result(0)?
        .into();
    let size_x_8 = block
        .append_operation(arith::muli(size, constant_8, location))
        .result(0)?
        .into();
    let dynamic_gas = block
        .append_operation(arith::addi(topic_count_x_375, size_x_8, location))
        .result(0)?
        .into();
    let dynamic_gas = block
        .append_operation(arith::trunci(dynamic_gas, uint64.into(), location))
        .result(0)?
        .into();
    Ok(dynamic_gas)
}

pub fn stack_pop<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    let uint256 = IntegerType::new(context, 256);
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);

    // Get address of stack pointer global
    let stack_ptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load stack pointer
    let stack_ptr = block
        .append_operation(llvm::load(
            context,
            stack_ptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?;

    // Decrement stack pointer
    let old_stack_ptr = block
        .append_operation(llvm::get_element_ptr(
            context,
            stack_ptr.into(),
            DenseI32ArrayAttribute::new(context, &[-1]),
            uint256.into(),
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load value from top of stack
    let value = block
        .append_operation(llvm::load(
            context,
            old_stack_ptr.into(),
            uint256.into(),
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    // Store decremented stack pointer
    let res = block.append_operation(llvm::store(
        context,
        old_stack_ptr.into(),
        stack_ptr_ptr.into(),
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    Ok(value)
}

pub fn constant_value_from_i64<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    value: i64,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    let location = Location::unknown(context);

    Ok(block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, value).into(),
            location,
        ))
        .result(0)?
        .into())
}

pub fn stack_push<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    value: Value,
) -> Result<(), CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);

    //Check that the value to push is 256 bits wide.
    let uint256 = IntegerType::new(context, 256);
    debug_assert!(value.r#type().eq(&uint256.into()));

    // Get address of stack pointer global
    let stack_ptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load stack pointer
    let stack_ptr = block
        .append_operation(llvm::load(
            context,
            stack_ptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?;

    // Store value at stack pointer
    let res = block.append_operation(llvm::store(
        context,
        value,
        stack_ptr.into(),
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    // Increment stack pointer
    let new_stack_ptr = block
        .append_operation(llvm::get_element_ptr(
            context,
            stack_ptr.into(),
            DenseI32ArrayAttribute::new(context, &[1]),
            uint256.into(),
            ptr_type,
            location,
        ))
        .result(0)?;

    // Store incremented stack pointer
    let res = block.append_operation(llvm::store(
        context,
        new_stack_ptr.into(),
        stack_ptr_ptr.into(),
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    Ok(())
}

// Returns a copy of the nth value of the stack along with its stack's address
pub fn get_nth_from_stack<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    nth: u8,
) -> Result<(Value<'ctx, 'ctx>, OperationResult<'ctx, 'ctx>), CodegenError> {
    debug_assert!((nth as u32) < MAX_STACK_SIZE as u32);
    let uint256 = IntegerType::new(context, 256);
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);

    // Get address of stack pointer global
    let stack_ptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load stack pointer
    let stack_ptr = block
        .append_operation(llvm::load(
            context,
            stack_ptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?;

    // Decrement stack pointer
    let nth_stack_ptr = block
        .append_operation(llvm::get_element_ptr(
            context,
            stack_ptr.into(),
            DenseI32ArrayAttribute::new(context, &[-(nth as i32)]),
            uint256.into(),
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load value from top of stack
    let value = block
        .append_operation(llvm::load(
            context,
            nth_stack_ptr.into(),
            uint256.into(),
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    Ok((value, nth_stack_ptr))
}

pub fn swap_stack_elements<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    position_1: u8,
    position_2: u8,
) -> Result<(), CodegenError> {
    debug_assert!((position_1 as u32) < MAX_STACK_SIZE as u32);
    debug_assert!((position_2 as u32) < MAX_STACK_SIZE as u32);
    let location = Location::unknown(context);

    let (first_element, first_elem_address) = get_nth_from_stack(context, block, position_1)?;
    let (nth_element, nth_elem_address) = get_nth_from_stack(context, block, position_2)?;

    // Store element in position 1 into position 2
    let res = block.append_operation(llvm::store(
        context,
        first_element,
        nth_elem_address.into(),
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    // Store element in position 2 into position 1
    let res = block.append_operation(llvm::store(
        context,
        nth_element,
        first_elem_address.into(),
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    Ok(())
}

/// Generates code for checking if the stack has enough space for `element_count` more elements.
pub fn check_stack_has_space_for<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    element_count: u32,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    debug_assert!(element_count < MAX_STACK_SIZE as u32);
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let uint256 = IntegerType::new(context, 256);

    // Get address of stack pointer global
    let stack_ptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load stack pointer
    let stack_ptr = block
        .append_operation(llvm::load(
            context,
            stack_ptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?;

    // Get address of stack base pointer global
    let stack_baseptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_BASEPTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load stack base pointer
    let stack_baseptr = block
        .append_operation(llvm::load(
            context,
            stack_baseptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?;

    // Compare `subtracted_stack_ptr = stack_ptr + element_count - MAX_STACK_SIZE`
    let subtracted_stack_ptr = block
        .append_operation(llvm::get_element_ptr(
            context,
            stack_ptr.into(),
            DenseI32ArrayAttribute::new(context, &[element_count as i32 - MAX_STACK_SIZE as i32]),
            uint256.into(),
            ptr_type,
            location,
        ))
        .result(0)?;

    // Compare `stack_ptr + element_count - MAX_STACK_SIZE <= stack_baseptr`
    let flag = block
        .append_operation(
            ods::llvm::icmp(
                context,
                IntegerType::new(context, 1).into(),
                subtracted_stack_ptr.into(),
                stack_baseptr.into(),
                // 7 should be the "ule" predicate enum value
                IntegerAttribute::new(
                    IntegerType::new(context, 64).into(),
                    /* "ule" predicate enum value */ 7,
                )
                .into(),
                location,
            )
            .into(),
        )
        .result(0)?;

    Ok(flag.into())
}

/// Generates code for checking if the stack has enough space for `element_count` more elements.
/// Returns true if there are at least `element_count` elements in the stack.
pub fn check_stack_has_at_least<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    element_count: u32,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    debug_assert!(element_count < MAX_STACK_SIZE as u32);
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let uint256 = IntegerType::new(context, 256);

    // Get address of stack pointer global
    let stack_ptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load stack pointer
    let stack_ptr = block
        .append_operation(llvm::load(
            context,
            stack_ptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?;

    // Get address of stack base pointer global
    let stack_baseptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_BASEPTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    // Load stack base pointer
    let stack_baseptr = block
        .append_operation(llvm::load(
            context,
            stack_baseptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?;

    // Compare `subtracted_stack_ptr = stack_ptr - element_count`
    let subtracted_stack_ptr = block
        .append_operation(llvm::get_element_ptr(
            context,
            stack_ptr.into(),
            DenseI32ArrayAttribute::new(context, &[-(element_count as i32)]),
            uint256.into(),
            ptr_type,
            location,
        ))
        .result(0)?;

    // Compare `stack_ptr - element_count >= stack_baseptr`
    let flag = block
        .append_operation(
            ods::llvm::icmp(
                context,
                IntegerType::new(context, 1).into(),
                subtracted_stack_ptr.into(),
                stack_baseptr.into(),
                IntegerAttribute::new(
                    IntegerType::new(context, 64).into(),
                    /* "uge" predicate enum value */ 9,
                )
                .into(),
                location,
            )
            .into(),
        )
        .result(0)?;

    Ok(flag.into())
}

pub fn compare_values<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    predicate: CmpiPredicate,
    lhs: Value<'ctx, 'ctx>,
    rhs: Value<'ctx, 'ctx>,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    let location = Location::unknown(context);

    let flag = block
        .append_operation(arith::cmpi(context, predicate, lhs, rhs, location))
        .result(0)?;

    Ok(flag.into())
}

pub fn check_if_zero<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    value: &'ctx Value,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    let location = Location::unknown(context);

    //Load zero value constant
    let zero_constant_value = block
        .append_operation(arith::constant(
            context,
            integer_constant_from_i64(context, 0i64).into(),
            location,
        ))
        .result(0)?
        .into();

    //Perform the comparisson -> value == 0
    let flag = block
        .append_operation(
            ods::llvm::icmp(
                context,
                IntegerType::new(context, 1).into(),
                zero_constant_value,
                *value,
                IntegerAttribute::new(
                    IntegerType::new(context, 64).into(),
                    /* "eq" predicate enum value */ 0,
                )
                .into(),
                location,
            )
            .into(),
        )
        .result(0)?;

    Ok(flag.into())
}

pub(crate) fn round_up_32<'c>(
    op_ctx: &'c OperationCtx,
    block: &'c Block,
    size: Value<'c, 'c>,
) -> Result<Value<'c, 'c>, CodegenError> {
    let context = op_ctx.mlir_context;
    let location = Location::unknown(context);
    let uint32 = IntegerType::new(context, 32).into();

    let constant_31 = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32, 31).into(),
            location,
        ))
        .result(0)?
        .into();

    let constant_32 = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32, 32).into(),
            location,
        ))
        .result(0)?
        .into();

    let size_plus_31 = block
        .append_operation(arith::addi(size, constant_31, location))
        .result(0)?
        .into();

    let memory_size_word = block
        .append_operation(arith::divui(size_plus_31, constant_32, location))
        .result(0)?
        .into();

    let memory_size_bytes = block
        .append_operation(arith::muli(memory_size_word, constant_32, location))
        .result(0)?
        .into();

    Ok(memory_size_bytes)
}

pub(crate) fn compute_copy_cost<'c>(
    op_ctx: &'c OperationCtx,
    block: &'c Block,
    memory_byte_size: Value<'c, 'c>,
) -> Result<Value<'c, 'c>, CodegenError> {
    // this function computes memory copying cost (excluding expansion), which is given by the following equations
    // memory_size_word = (memory_byte_size + 31) / 32
    // memory_cost = 3 * memory_size_word
    //
    //
    let context = op_ctx.mlir_context;
    let location = Location::unknown(context);
    let uint64 = IntegerType::new(context, 64).into();

    let memory_size_extended = block
        .append_operation(arith::extui(memory_byte_size, uint64, location))
        .result(0)?
        .into();

    let constant_3 = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint64, 3).into(),
            location,
        ))
        .result(0)?
        .into();

    let constant_31 = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint64, 31).into(),
            location,
        ))
        .result(0)?
        .into();

    let constant_32 = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint64, 32).into(),
            location,
        ))
        .result(0)?
        .into();

    let memory_byte_size_plus_31 = block
        .append_operation(arith::addi(memory_size_extended, constant_31, location))
        .result(0)?
        .into();

    let memory_size_word = block
        .append_operation(arith::divui(
            memory_byte_size_plus_31,
            constant_32,
            location,
        ))
        .result(0)?
        .into();

    let memory_cost = block
        .append_operation(arith::muli(memory_size_word, constant_3, location))
        .result(0)?
        .into();

    Ok(memory_cost)
}

pub(crate) fn compute_memory_cost<'c>(
    op_ctx: &'c OperationCtx,
    block: &'c Block,
    memory_byte_size: Value<'c, 'c>,
) -> Result<Value<'c, 'c>, CodegenError> {
    // this function computes memory cost, which is given by the following equations
    // memory_size_word = (memory_byte_size + 31) / 32
    // memory_cost = (memory_size_word ** 2) / 512 + (3 * memory_size_word)
    //
    //
    let context = op_ctx.mlir_context;
    let location = Location::unknown(context);
    let uint64 = IntegerType::new(context, 64).into();

    let memory_size_extended = block
        .append_operation(arith::extui(memory_byte_size, uint64, location))
        .result(0)?
        .into();

    let constant_31 = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint64, 31).into(),
            location,
        ))
        .result(0)?
        .into();

    let constant_512 = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint64, 512).into(),
            location,
        ))
        .result(0)?
        .into();

    let constant_32 = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint64, 32).into(),
            location,
        ))
        .result(0)?
        .into();

    let constant_3 = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint64, 3).into(),
            location,
        ))
        .result(0)?
        .into();

    let memory_byte_size_plus_31 = block
        .append_operation(arith::addi(memory_size_extended, constant_31, location))
        .result(0)?
        .into();

    let memory_size_word = block
        .append_operation(arith::divui(
            memory_byte_size_plus_31,
            constant_32,
            location,
        ))
        .result(0)?
        .into();

    let memory_size_word_squared = block
        .append_operation(arith::muli(memory_size_word, memory_size_word, location))
        .result(0)?
        .into();

    let memory_size_word_squared_divided_by_512 = block
        .append_operation(arith::divui(
            memory_size_word_squared,
            constant_512,
            location,
        ))
        .result(0)?
        .into();

    let memory_size_word_times_3 = block
        .append_operation(arith::muli(memory_size_word, constant_3, location))
        .result(0)?
        .into();

    let memory_cost = block
        .append_operation(arith::addi(
            memory_size_word_squared_divided_by_512,
            memory_size_word_times_3,
            location,
        ))
        .result(0)?
        .into();

    Ok(memory_cost)
}

pub(crate) fn get_calldata_ptr<'c>(
    op_ctx: &'c OperationCtx,
    block: &'c Block,
    location: Location<'c>,
) -> Result<Value<'c, 'c>, CodegenError> {
    let context = op_ctx.mlir_context;
    let ptr_type = pointer(context, 0);

    let calldata_ptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            CALLDATA_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    let calldata_ptr = block
        .append_operation(llvm::load(
            context,
            calldata_ptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    Ok(calldata_ptr)
}

pub(crate) fn get_calldata_size<'c>(
    op_ctx: &'c OperationCtx,
    block: &'c Block,
    location: Location<'c>,
) -> Result<Value<'c, 'c>, CodegenError> {
    let context = op_ctx.mlir_context;
    let ptr_type = pointer(context, 0);

    let calldata_size_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            CALLDATA_SIZE_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    let calldata_size = block
        .append_operation(llvm::load(
            context,
            calldata_size_ptr.into(),
            IntegerType::new(context, 32).into(),
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    Ok(calldata_size)
}

/// Wrapper for calling the [`extend_memory`](crate::syscall::SyscallContext::extend_memory) syscall.
/// Extends memory only if the current memory size is less than the required size, consuming the corresponding gas.
pub(crate) fn extend_memory<'c>(
    op_ctx: &'c OperationCtx,
    block: &'c Block,
    finish_block: &'c Block,
    region: &Region<'c>,
    required_size: Value<'c, 'c>,
    fixed_gas: i64,
) -> Result<(), CodegenError> {
    let context = op_ctx.mlir_context;
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let uint32 = IntegerType::new(context, 32);
    let uint64 = IntegerType::new(context, 64);

    // Load memory size
    let memory_size_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            MEMORY_SIZE_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?
        .into();
    let memory_size = block
        .append_operation(llvm::load(
            context,
            memory_size_ptr,
            uint32.into(),
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    let rounded_required_size = round_up_32(op_ctx, block, required_size)?;

    // Compare current memory size and required size
    let extension_flag = compare_values(
        context,
        block,
        CmpiPredicate::Ult,
        memory_size,
        rounded_required_size,
    )?;
    let extension_block = region.append_block(Block::new(&[]));
    let no_extension_block = region.append_block(Block::new(&[]));

    block.append_operation(cf::cond_br(
        context,
        extension_flag,
        &extension_block,
        &no_extension_block,
        &[],
        &[],
        location,
    ));

    // Consume gas for memory extension case
    let memory_cost_before = compute_memory_cost(op_ctx, &extension_block, memory_size)?;
    let memory_cost_after = compute_memory_cost(op_ctx, &extension_block, rounded_required_size)?;

    let dynamic_gas_value = extension_block
        .append_operation(arith::subi(memory_cost_after, memory_cost_before, location))
        .result(0)?
        .into();
    let fixed_gas_value = extension_block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint64.into(), fixed_gas).into(),
            location,
        ))
        .result(0)?
        .into();
    let total_gas = extension_block
        .append_operation(arith::addi(dynamic_gas_value, fixed_gas_value, location))
        .result(0)?
        .into();
    let extension_gas_flag = consume_gas_as_value(context, &extension_block, total_gas)?;

    // Consume gas for no memory extension case
    let no_extension_gas_flag = consume_gas(context, &no_extension_block, fixed_gas)?;

    let memory_ptr =
        op_ctx.extend_memory_syscall(&extension_block, rounded_required_size, location)?;

    // Store new memory size and pointer
    let res = extension_block.append_operation(llvm::store(
        context,
        rounded_required_size,
        memory_size_ptr,
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());
    let memory_ptr_ptr = extension_block
        .append_operation(llvm_mlir::addressof(
            context,
            MEMORY_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;
    let res = extension_block.append_operation(llvm::store(
        context,
        memory_ptr,
        memory_ptr_ptr.into(),
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    // Jump to finish block
    extension_block.append_operation(cf::cond_br(
        context,
        extension_gas_flag,
        finish_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    no_extension_block.append_operation(cf::cond_br(
        context,
        no_extension_gas_flag,
        finish_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    Ok(())
}

pub(crate) fn get_memory_pointer<'a>(
    op_ctx: &'a OperationCtx<'a>,
    block: &'a Block<'a>,
    location: Location<'a>,
) -> Result<Value<'a, 'a>, CodegenError> {
    let context = op_ctx.mlir_context;
    let ptr_type = pointer(context, 0);

    let memory_ptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            MEMORY_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    let memory_ptr = block
        .append_operation(llvm::load(
            context,
            memory_ptr_ptr.into(),
            ptr_type,
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?;

    Ok(memory_ptr.into())
}

pub(crate) fn return_empty_result(
    op_ctx: &OperationCtx,
    block: &Block,
    reason_code: ExitStatusCode,
    location: Location,
) -> Result<(), CodegenError> {
    let context = op_ctx.mlir_context;
    let uint32 = IntegerType::new(context, 32).into();

    let zero_constant = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32, 0).into(),
            location,
        ))
        .result(0)?
        .into();

    return_result_with_offset_and_size(
        op_ctx,
        block,
        zero_constant,
        zero_constant,
        reason_code,
        location,
    )?;

    Ok(())
}

pub(crate) fn return_result_from_stack(
    op_ctx: &OperationCtx,
    region: &Region<'_>,
    block: &Block,
    reason_code: ExitStatusCode,
    location: Location,
) -> Result<(), CodegenError> {
    let context = op_ctx.mlir_context;
    let uint32 = IntegerType::new(context, 32);

    let offset_u256 = stack_pop(context, block)?;
    let size_u256 = stack_pop(context, block)?;

    let offset = block
        .append_operation(arith::trunci(offset_u256, uint32.into(), location))
        .result(0)
        .unwrap()
        .into();

    let size = block
        .append_operation(arith::trunci(size_u256, uint32.into(), location))
        .result(0)
        .unwrap()
        .into();

    let required_size = block
        .append_operation(arith::addi(offset, size, location))
        .result(0)?
        .into();

    let return_block = region.append_block(Block::new(&[]));

    extend_memory(op_ctx, block, &return_block, region, required_size, 0)?;

    return_result_with_offset_and_size(op_ctx, &return_block, offset, size, reason_code, location)?;

    Ok(())
}

pub(crate) fn return_result_with_offset_and_size(
    op_ctx: &OperationCtx,
    block: &Block,
    offset: Value,
    size: Value,
    reason_code: ExitStatusCode,
    location: Location,
) -> Result<(), CodegenError> {
    let context = op_ctx.mlir_context;
    let remaining_gas = get_remaining_gas(context, block)?;

    let reason = block
        .append_operation(arith::constant(
            context,
            integer_constant_from_u8(context, reason_code.to_u8()).into(),
            location,
        ))
        .result(0)?
        .into();

    op_ctx.write_result_syscall(block, offset, size, remaining_gas, reason, location);

    block.append_operation(func::r#return(&[reason], location));
    Ok(())
}

pub(crate) fn get_block_number<'a>(
    op_ctx: &'a OperationCtx<'a>,
    block: &'a Block<'a>,
) -> Result<Value<'a, 'a>, CodegenError> {
    let context = op_ctx.mlir_context;
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let pointer_size = constant_value_from_i64(context, block, 1_i64)?;
    let uint256 = IntegerType::new(context, 256);

    let block_number_ptr = block
        .append_operation(llvm::alloca(
            context,
            pointer_size,
            ptr_type,
            location,
            AllocaOptions::new().elem_type(Some(TypeAttribute::new(uint256.into()))),
        ))
        .result(0)?
        .into();

    op_ctx.get_block_number_syscall(block, block_number_ptr, location);

    // get the value from the pointer
    let block_number = block
        .append_operation(llvm::load(
            context,
            block_number_ptr,
            IntegerType::new(context, 256).into(),
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    Ok(block_number)
}

pub(crate) fn get_prevrandao<'a>(
    op_ctx: &'a OperationCtx<'a>,
    block: &'a Block<'a>,
) -> Result<Value<'a, 'a>, CodegenError> {
    let context = op_ctx.mlir_context;
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let pointer_size = constant_value_from_i64(context, block, 1_i64)?;
    let uint256 = IntegerType::new(context, 256);

    let prevrandao_ptr = block
        .append_operation(llvm::alloca(
            context,
            pointer_size,
            ptr_type,
            location,
            AllocaOptions::new().elem_type(Some(TypeAttribute::new(uint256.into()))),
        ))
        .result(0)?
        .into();

    op_ctx.get_prevrandao_syscall(block, prevrandao_ptr, location);

    // get the value from the pointer
    let prevrandao = block
        .append_operation(llvm::load(
            context,
            prevrandao_ptr,
            IntegerType::new(context, 256).into(),
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    Ok(prevrandao)
}

pub(crate) fn get_blob_hash_at_index<'a>(
    op_ctx: &'a OperationCtx<'a>,
    block: &'a Block<'a>,
    index_ptr: Value<'a, 'a>,
) -> Result<Value<'a, 'a>, CodegenError> {
    let context = op_ctx.mlir_context;
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let pointer_size = constant_value_from_i64(context, block, 1_i64)?;
    let uint256 = IntegerType::new(context, 256);

    let blobhash_ptr = block
        .append_operation(llvm::alloca(
            context,
            pointer_size,
            ptr_type,
            location,
            AllocaOptions::new().elem_type(Some(TypeAttribute::new(uint256.into()))),
        ))
        .result(0)?
        .into();

    op_ctx.get_blob_hash_at_index_syscall(block, index_ptr, blobhash_ptr, location);

    // get the value from the pointer
    let blobhash = block
        .append_operation(llvm::load(
            context,
            blobhash_ptr,
            IntegerType::new(context, 256).into(),
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    Ok(blobhash)
}

pub fn integer_constant_from_i64(context: &MeliorContext, value: i64) -> IntegerAttribute {
    let uint256 = IntegerType::new(context, 256);
    IntegerAttribute::new(uint256.into(), value)
}

pub fn integer_constant_from_u8(context: &MeliorContext, value: u8) -> IntegerAttribute {
    let uint8 = IntegerType::new(context, 8);
    IntegerAttribute::new(uint8.into(), value.into())
}
/// Allocates memory for a 32-byte value, stores the value in the memory
/// and returns a pointer to the value
pub(crate) fn allocate_and_store_value<'a>(
    op_ctx: &'a OperationCtx<'a>,
    block: &'a Block<'a>,
    value: Value<'a, 'a>,
    location: Location<'a>,
) -> Result<Value<'a, 'a>, CodegenError> {
    let context = op_ctx.mlir_context;
    let ptr_type = pointer(context, 0);
    let uint32 = IntegerType::new(context, 32);
    let uint256 = IntegerType::new(context, 256);

    let number_of_elements = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32.into(), 1).into(),
            location,
        ))
        .result(0)?
        .into();

    let value_ptr = block
        .append_operation(llvm::alloca(
            context,
            number_of_elements,
            ptr_type,
            location,
            AllocaOptions::new().elem_type(TypeAttribute::new(uint256.into()).into()),
        ))
        .result(0)?
        .into();

    block.append_operation(llvm::store(
        context,
        value,
        value_ptr,
        location,
        LoadStoreOptions::default()
            .align(IntegerAttribute::new(IntegerType::new(context, 64).into(), 1).into()),
    ));

    Ok(value_ptr)
}

/// Returns the basefee
pub(crate) fn get_basefee<'a>(
    op_ctx: &'a OperationCtx<'a>,
    block: &'a Block<'a>,
) -> Result<Value<'a, 'a>, CodegenError> {
    let context = op_ctx.mlir_context;
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let pointer_size = constant_value_from_i64(context, block, 1_i64)?;
    let uint256 = IntegerType::new(context, 256);

    let basefee_ptr = block
        .append_operation(llvm::alloca(
            context,
            pointer_size,
            ptr_type,
            location,
            AllocaOptions::new().elem_type(Some(TypeAttribute::new(uint256.into()))),
        ))
        .result(0)?
        .into();

    op_ctx.store_in_basefee_ptr_syscall(basefee_ptr, block, location);

    // get the value from the pointer
    let basefee = block
        .append_operation(llvm::load(
            context,
            basefee_ptr,
            IntegerType::new(context, 256).into(),
            location,
            LoadStoreOptions::default(),
        ))
        .result(0)?
        .into();

    Ok(basefee)
}

/// Calculates the blob gas price from the header's excess blob gas field.
///
/// See also [the EIP-4844 helpers](https://eips.ethereum.org/EIPS/eip-4844#helpers)
/// (`get_blob_gasprice`).
pub fn calc_blob_gasprice(excess_blob_gas: u64) -> u128 {
    fake_exponential(
        gas_cost::MIN_BLOB_GASPRICE,
        excess_blob_gas,
        gas_cost::BLOB_GASPRICE_UPDATE_FRACTION,
    )
}

/// Approximates `factor * e ** (numerator / denominator)` using Taylor expansion.
///
/// This is used to calculate the blob price.
///
/// See also [the EIP-4844 helpers](https://eips.ethereum.org/EIPS/eip-4844#helpers)
/// (`fake_exponential`).
///
/// # Panics
///
/// This function panics if `denominator` is zero.
pub fn fake_exponential(factor: u64, numerator: u64, denominator: u64) -> u128 {
    assert_ne!(denominator, 0, "attempt to divide by zero");
    let factor = factor as u128;
    let numerator = numerator as u128;
    let denominator = denominator as u128;

    let mut i = 1;
    let mut output = 0;
    let mut numerator_accum = factor * denominator;
    while numerator_accum > 0 {
        output += numerator_accum;

        // Denominator is asserted as not zero at the start of the function.
        numerator_accum = (numerator_accum * numerator) / (denominator * i);
        i += 1;
    }
    output / denominator
}

pub mod llvm_mlir {
    use melior::{
        dialect::llvm::{self, attributes::Linkage},
        ir::{
            attribute::{FlatSymbolRefAttribute, StringAttribute, TypeAttribute},
            operation::OperationBuilder,
            Identifier, Location, Region,
        },
        Context as MeliorContext,
    };

    pub fn global<'c>(
        context: &'c MeliorContext,
        name: &str,
        global_type: melior::ir::Type<'c>,
        linkage: Linkage,
        location: Location<'c>,
    ) -> melior::ir::Operation<'c> {
        // TODO: use ODS
        OperationBuilder::new("llvm.mlir.global", location)
            .add_regions([Region::new()])
            .add_attributes(&[
                (
                    Identifier::new(context, "sym_name"),
                    StringAttribute::new(context, name).into(),
                ),
                (
                    Identifier::new(context, "global_type"),
                    TypeAttribute::new(global_type).into(),
                ),
                (
                    Identifier::new(context, "linkage"),
                    llvm::attributes::linkage(context, linkage),
                ),
            ])
            .build()
            .expect("valid operation")
    }

    pub fn addressof<'c>(
        context: &'c MeliorContext,
        name: &str,
        result_type: melior::ir::Type<'c>,
        location: Location<'c>,
    ) -> melior::ir::Operation<'c> {
        // TODO: use ODS
        OperationBuilder::new("llvm.mlir.addressof", location)
            .add_attributes(&[(
                Identifier::new(context, "global_name"),
                FlatSymbolRefAttribute::new(context, name).into(),
            )])
            .add_results(&[result_type])
            .build()
            .expect("valid operation")
    }
}

pub fn encode_rlp_u64(number: u64) -> Bytes {
    let mut buf: Vec<u8> = vec![];
    match number {
        // 0, also known as null or the empty string is 0x80
        0 => buf.put_u8(0x80),
        // for a single byte whose value is in the [0x00, 0x7f] range, that byte is its own RLP encoding.
        n @ 1..=0x7f => buf.put_u8(n as u8),
        // Otherwise, if a string is 0-55 bytes long, the RLP encoding consists of a
        // single byte with value RLP_NULL (0x80) plus the length of the string followed by the string.
        n => {
            let mut bytes: Vec<u8> = vec![];
            bytes.extend_from_slice(&n.to_be_bytes());
            let start = bytes.iter().position(|&x| x != 0).unwrap();
            let len = bytes.len() - start;
            buf.put_u8(0x80 + len as u8);
            buf.put_slice(&bytes[start..]);
        }
    }
    buf.into()
}

pub fn compute_contract_address(address: H160, nonce: u64) -> Address {
    // Compute the destination address as keccak256(rlp([sender_address,sender_nonce]))[12:]
    // TODO: replace manual encoding once rlp is added
    let encoded_nonce = encode_rlp_u64(nonce);
    let mut buf = Vec::<u8>::new();
    buf.push(0xd5);
    buf.extend_from_slice(&encoded_nonce.len().to_be_bytes());
    buf.push(0x94);
    buf.extend_from_slice(address.as_bytes());
    buf.extend_from_slice(&encoded_nonce);
    let mut hasher = Keccak256::new();
    hasher.update(&buf);
    Address::from_slice(&hasher.finalize()[12..])
}

pub fn compute_contract_address2(address: H160, salt: U256, initialization_code: &[u8]) -> Address {
    // Compute the destination address as keccak256(0xff + sender_address + salt + keccak256(initialisation_code))[12:]
    let mut hasher = Keccak256::new();
    hasher.update(initialization_code);
    let initialization_code_hash = hasher.finalize();

    let mut hasher = Keccak256::new();
    let mut salt_bytes = [0; 32];
    salt.to_big_endian(&mut salt_bytes);
    hasher.update([0xff]);
    hasher.update(address.as_bytes());
    hasher.update(salt_bytes);
    hasher.update(initialization_code_hash);
    Address::from_slice(&hasher.finalize()[12..])
}
