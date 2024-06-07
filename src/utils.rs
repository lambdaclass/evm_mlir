use melior::{
    dialect::{
        arith, cf, func,
        llvm::{self, r#type::pointer, LoadStoreOptions},
        ods,
    },
    ir::{
        attribute::{DenseI32ArrayAttribute, IntegerAttribute},
        operation::OperationResult,
        r#type::IntegerType,
        Block, Location, Region, Value,
    },
    Context as MeliorContext,
};

use crate::{
    codegen::context::OperationCtx,
    constants::{
        GAS_COUNTER_GLOBAL, MAX_STACK_SIZE, MEMORY_PTR_GLOBAL, MEMORY_SIZE_GLOBAL,
        REVERT_EXIT_CODE, STACK_BASEPTR_GLOBAL, STACK_PTR_GLOBAL,
    },
    errors::CodegenError,
};

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

/// Returns true if there is enough Gas
pub fn consume_gas_as_value<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    gas_value: Value<'ctx, 'ctx>,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let uint32 = IntegerType::new(context, 32).into();

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
            uint32,
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
    nth: u32,
) -> Result<(Value<'ctx, 'ctx>, OperationResult<'ctx, 'ctx>), CodegenError> {
    debug_assert!(nth < MAX_STACK_SIZE as u32);
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
    position_1: u32,
    position_2: u32,
) -> Result<(), CodegenError> {
    debug_assert!(position_1 < MAX_STACK_SIZE as u32);
    debug_assert!(position_2 < MAX_STACK_SIZE as u32);
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

// Generates code for checking if lhs < rhs
pub fn check_is_greater_than<'ctx>(
    context: &'ctx MeliorContext,
    block: &'ctx Block,
    lhs: Value<'ctx, 'ctx>,
    rhs: Value<'ctx, 'ctx>,
) -> Result<Value<'ctx, 'ctx>, CodegenError> {
    let location = Location::unknown(context);

    let flag = block
        .append_operation(arith::cmpi(
            context,
            arith::CmpiPredicate::Ugt,
            rhs,
            lhs,
            location,
        ))
        .result(0)?;

    Ok(flag.into())
}

pub fn generate_revert_block(context: &MeliorContext) -> Result<Block, CodegenError> {
    // TODO: return result via write_result syscall
    let location = Location::unknown(context);
    let uint8 = IntegerType::new(context, 8);

    let revert_block = Block::new(&[]);

    let constant_value = IntegerAttribute::new(uint8.into(), REVERT_EXIT_CODE as _).into();

    let exit_code = revert_block
        .append_operation(arith::constant(context, constant_value, location))
        .result(0)?
        .into();

    revert_block.append_operation(func::r#return(&[exit_code], location));
    Ok(revert_block)
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
    let uint32 = IntegerType::new(context, 32).into();

    // Esta bien usar 32 bits para el cost? sino usar 64 y extender memory_byte_size

    let thirty_one = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32, 31).into(),
            location,
        ))
        .result(0)?
        .into();

    let five_hundred_and_twelve = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32, 512).into(),
            location,
        ))
        .result(0)?
        .into();

    let thirty_two = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32, 32).into(),
            location,
        ))
        .result(0)?
        .into();

    let three = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32, 3).into(),
            location,
        ))
        .result(0)?
        .into();

    let memory_byte_size_plus_31 = block
        .append_operation(arith::addi(memory_byte_size, thirty_one, location))
        .result(0)?
        .into();

    let memory_size_word = block
        .append_operation(arith::divui(memory_byte_size_plus_31, thirty_two, location))
        .result(0)?
        .into();

    let memory_size_word_squared = block
        .append_operation(arith::muli(memory_size_word, memory_size_word, location))
        .result(0)?
        .into();

    let memory_size_word_squared_divided_by_512 = block
        .append_operation(arith::divui(
            memory_size_word_squared,
            five_hundred_and_twelve,
            location,
        ))
        .result(0)?
        .into();

    let memory_size_word_x_3 = block
        .append_operation(arith::muli(memory_size_word, three, location))
        .result(0)?
        .into();

    let memory_cost = block
        .append_operation(arith::addi(
            memory_size_word_squared_divided_by_512,
            memory_size_word_x_3,
            location,
        ))
        .result(0)?
        .into();

    Ok(memory_cost)
}

/// Wrapper for calling the [`extend_memory`](crate::syscall::SyscallContext::extend_memory) syscall.
/// Extends memory only if the current memory size is less than the required size, consuming the corresponding gas.
pub(crate) fn extend_memory<'c>(
    op_ctx: &'c OperationCtx,
    block: &'c Block,
    finish_block: &'c Block,
    region: &Region<'c>,
    required_size: Value<'c, 'c>,
    static_gas: i64,
) -> Result<(), CodegenError> {
    let context = op_ctx.mlir_context;
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let uint32 = IntegerType::new(context, 32);

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

    // Compare current memory size and required size
    let extension_flag = check_is_greater_than(context, block, memory_size, required_size)?;
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

    // Extend memory
    let memory_ptr = op_ctx.extend_memory_syscall(&extension_block, required_size, location)?; // TODO: memory extension should be in 32 byte words?

    // Store new memory size and pointer
    let res = extension_block.append_operation(llvm::store(
        context,
        required_size,
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

    // Consume gas for memory extension case
    let memory_cost_before = compute_memory_cost(op_ctx, &extension_block, memory_size)?;
    let memory_cost_after = compute_memory_cost(op_ctx, &extension_block, required_size)?;

    let dynamic_gas_value = extension_block
        .append_operation(arith::subi(memory_cost_after, memory_cost_before, location))
        .result(0)?
        .into();
    let static_gas_value = extension_block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32.into(), static_gas).into(),
            location,
        ))
        .result(0)?
        .into();
    let total_gas = extension_block
        .append_operation(arith::addi(dynamic_gas_value, static_gas_value, location))
        .result(0)?
        .into();
    let gas_flag = consume_gas_as_value(context, &extension_block, total_gas)?;

    extension_block.append_operation(cf::cond_br(
        context,
        gas_flag,
        finish_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    // Consume gas for no memory extension case
    let gas_flag = consume_gas(context, &no_extension_block, static_gas)?;

    no_extension_block.append_operation(cf::cond_br(
        context,
        gas_flag,
        finish_block,
        &op_ctx.revert_block,
        &[],
        &[],
        location,
    ));

    // TODO: cargar memory_ptr en caso de extension se puede ahorrar, pasandolo como parametro al finish_block/
    Ok(())
}

pub fn integer_constant_from_i64(context: &MeliorContext, value: i64) -> IntegerAttribute {
    let uint256 = IntegerType::new(context, 256);
    IntegerAttribute::new(uint256.into(), value)
}

pub fn integer_constant_from_u8(context: &MeliorContext, value: u8) -> IntegerAttribute {
    let uint8 = IntegerType::new(context, 8);
    IntegerAttribute::new(uint8.into(), value.into())
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
                    llvm::attributes::linkage(context, Linkage::Internal),
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
