use std::collections::BTreeMap;

use melior::{
    dialect::{
        arith, cf, func,
        llvm::{self, r#type::pointer, AllocaOptions, LoadStoreOptions},
    },
    ir::{
        attribute::{IntegerAttribute, TypeAttribute},
        r#type::IntegerType,
        Block, BlockRef, Location, Module, Region, Value,
    },
    Context as MeliorContext,
};

use crate::{
    constants::{
        GAS_COUNTER_GLOBAL, MAX_STACK_SIZE, MEMORY_PTR_GLOBAL, MEMORY_SIZE_GLOBAL,
        STACK_BASEPTR_GLOBAL, STACK_PTR_GLOBAL,
    },
    errors::CodegenError,
    program::{Operation, Program},
    syscall::{self, ExitStatusCode},
    utils::{get_remaining_gas, integer_constant_from_u8, llvm_mlir},
};

#[derive(Debug, Clone)]
pub(crate) struct OperationCtx<'c> {
    /// The MLIR context.
    pub mlir_context: &'c MeliorContext,
    /// The program IR.
    pub program: &'c Program,
    /// The syscall context to be passed to syscalls.
    pub syscall_ctx: Value<'c, 'c>,
    /// Reference to the revert block.
    /// This block takes care of reverts.
    pub revert_block: BlockRef<'c, 'c>,
    /// Reference to the jump table block.
    /// This block receives the PC as an argument and jumps to the block corresponding to that PC,
    /// or reverts in case the destination is not a JUMPDEST.
    pub jumptable_block: BlockRef<'c, 'c>,
    /// Blocks to jump to. These are registered dynamically as JUMPDESTs are processed.
    pub jumpdest_blocks: BTreeMap<usize, BlockRef<'c, 'c>>,
}

impl<'c> OperationCtx<'c> {
    pub(crate) fn new(
        context: &'c MeliorContext,
        module: &'c Module,
        region: &'c Region,
        setup_block: &'c Block<'c>,
        program: &'c Program,
    ) -> Result<Self, CodegenError> {
        let location = Location::unknown(context);
        let ptr_type = pointer(context, 0);
        let uint64 = IntegerType::new(context, 64).into();
        // PERF: avoid generating unneeded setup blocks
        let syscall_ctx = setup_block.add_argument(ptr_type, location);
        let initial_gas = setup_block.add_argument(uint64, location);

        // Append setup code to be run at the start
        generate_stack_setup_code(context, module, setup_block)?;
        generate_memory_setup_code(context, module, setup_block)?;
        generate_gas_counter_setup_code(context, module, setup_block, initial_gas)?;

        syscall::mlir::declare_syscalls(context, module);

        // Generate helper blocks
        let revert_block = region.append_block(generate_revert_block(context, syscall_ctx)?);
        let jumptable_block = region.append_block(create_jumptable_landing_block(context));

        let op_ctx = OperationCtx {
            mlir_context: context,
            program,
            syscall_ctx,
            revert_block,
            jumptable_block,
            jumpdest_blocks: Default::default(),
        };
        Ok(op_ctx)
    }

    /// Populate the jumptable block with a dynamic dispatch according to the
    /// received PC.
    pub(crate) fn populate_jumptable(&self) -> Result<(), CodegenError> {
        let context = self.mlir_context;
        let program = self.program;
        let start_block = self.jumptable_block;

        let location = Location::unknown(context);
        let uint256 = IntegerType::new(context, 256);

        // The block receives a single argument: the value to switch on
        // TODO: move to program module
        let jumpdest_pcs: Vec<i64> = program
            .operations
            .iter()
            .filter_map(|op| match op {
                Operation::Jumpdest { pc } => Some(*pc as i64),
                _ => None,
            })
            .collect();

        let arg = start_block.argument(0)?;

        let case_destinations: Vec<_> = self
            .jumpdest_blocks
            .values()
            .map(|b| {
                let x: (&Block, &[Value]) = (b, &[]);
                x
            })
            .collect();

        let op = start_block.append_operation(cf::switch(
            context,
            &jumpdest_pcs,
            arg.into(),
            uint256.into(),
            (&self.revert_block, &[]),
            &case_destinations,
            location,
        )?);

        assert!(op.verify());

        Ok(())
    }

    /// Registers a block as a valid jump destination.
    // TODO: move into jumptable module
    pub(crate) fn register_jump_destination(&mut self, pc: usize, block: BlockRef<'c, 'c>) {
        self.jumpdest_blocks.insert(pc, block);
    }

    /// Registers a block as a valid jump destination.
    // TODO: move into jumptable module
    #[allow(dead_code)]
    pub(crate) fn add_jump_op(
        &mut self,
        block: BlockRef<'c, 'c>,
        pc_to_jump_to: Value,
        location: Location,
    ) {
        let op = block.append_operation(cf::br(&self.jumptable_block, &[pc_to_jump_to], location));
        assert!(op.verify());
    }
}

fn generate_gas_counter_setup_code<'c>(
    context: &'c MeliorContext,
    module: &'c Module,
    block: &'c Block<'c>,
    initial_gas: Value,
) -> Result<(), CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let uint64 = IntegerType::new(context, 64).into();

    let body = module.body();
    let res = body.append_operation(llvm_mlir::global(
        context,
        GAS_COUNTER_GLOBAL,
        uint64,
        location,
    ));

    assert!(res.verify());

    let gas_addr = block
        .append_operation(llvm_mlir::addressof(
            context,
            GAS_COUNTER_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    let res = block.append_operation(llvm::store(
        context,
        initial_gas,
        gas_addr.into(),
        location,
        LoadStoreOptions::default(),
    ));

    assert!(res.verify());

    Ok(())
}

fn generate_stack_setup_code<'c>(
    context: &'c MeliorContext,
    module: &'c Module,
    block: &'c Block<'c>,
) -> Result<(), CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);

    // Declare the stack pointer and base pointer globals
    let body = module.body();
    let res = body.append_operation(llvm_mlir::global(
        context,
        STACK_BASEPTR_GLOBAL,
        ptr_type,
        location,
    ));
    assert!(res.verify());
    let res = body.append_operation(llvm_mlir::global(
        context,
        STACK_PTR_GLOBAL,
        ptr_type,
        location,
    ));
    assert!(res.verify());

    let uint256 = IntegerType::new(context, 256);

    // Allocate stack memory
    let stack_size = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint256.into(), MAX_STACK_SIZE as i64).into(),
            location,
        ))
        .result(0)?
        .into();

    let stack_baseptr = block
        .append_operation(llvm::alloca(
            context,
            stack_size,
            ptr_type,
            location,
            AllocaOptions::new().elem_type(Some(TypeAttribute::new(uint256.into()))),
        ))
        .result(0)?;

    // Populate the globals with the allocated stack memory
    let stack_baseptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_BASEPTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    let res = block.append_operation(llvm::store(
        context,
        stack_baseptr.into(),
        stack_baseptr_ptr.into(),
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    let stackptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            STACK_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    let res = block.append_operation(llvm::store(
        context,
        stack_baseptr.into(),
        stackptr_ptr.into(),
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    Ok(())
}

fn generate_memory_setup_code<'c>(
    context: &'c MeliorContext,
    module: &'c Module,
    block: &'c Block<'c>,
) -> Result<(), CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let uint32 = IntegerType::new(context, 32).into();

    // Declare the stack pointer and base pointer globals
    let body = module.body();
    let res = body.append_operation(llvm_mlir::global(
        context,
        MEMORY_PTR_GLOBAL,
        ptr_type,
        location,
    ));
    assert!(res.verify());
    let res = body.append_operation(llvm_mlir::global(
        context,
        MEMORY_SIZE_GLOBAL,
        uint32,
        location,
    ));
    assert!(res.verify());

    let zero = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32, 0).into(),
            location,
        ))
        .result(0)?
        .into();

    let memory_size_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            MEMORY_SIZE_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?;

    let res = block.append_operation(llvm::store(
        context,
        zero,
        memory_size_ptr.into(),
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    Ok(())
}

/// Create the jumptable landing block. This is the main entrypoint
/// for JUMP and JUMPI operations.
fn create_jumptable_landing_block(context: &MeliorContext) -> Block {
    let location = Location::unknown(context);
    let uint256 = IntegerType::new(context, 256);
    Block::new(&[(uint256.into(), location)])
}

pub fn generate_revert_block<'c>(
    context: &'c MeliorContext,
    syscall_ctx: Value<'c, 'c>,
) -> Result<Block<'c>, CodegenError> {
    let location = Location::unknown(context);
    let uint32 = IntegerType::new(context, 32).into();

    let revert_block = Block::new(&[]);
    let remaining_gas = get_remaining_gas(context, &revert_block)?;

    let zero_constant = revert_block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint32, 0).into(),
            location,
        ))
        .result(0)?
        .into();

    let reason = revert_block
        .append_operation(arith::constant(
            context,
            integer_constant_from_u8(context, ExitStatusCode::Error.to_u8()).into(),
            location,
        ))
        .result(0)?
        .into();

    syscall::mlir::write_result_syscall(
        context,
        syscall_ctx,
        &revert_block,
        zero_constant,
        zero_constant,
        remaining_gas,
        reason,
        location,
    );

    revert_block.append_operation(func::r#return(&[reason], location));

    Ok(revert_block)
}

// Syscall MLIR wrappers
impl<'c> OperationCtx<'c> {
    pub(crate) fn write_result_syscall(
        &self,
        block: &Block,
        offset: Value,
        size: Value,
        gas: Value,
        reason: Value,
        location: Location,
    ) {
        syscall::mlir::write_result_syscall(
            self.mlir_context,
            self.syscall_ctx,
            block,
            offset,
            size,
            gas,
            reason,
            location,
        )
    }

    pub(crate) fn extend_memory_syscall(
        &'c self,
        block: &'c Block,
        new_size: Value<'c, 'c>,
        location: Location<'c>,
    ) -> Result<Value, CodegenError> {
        syscall::mlir::extend_memory_syscall(
            self.mlir_context,
            self.syscall_ctx,
            block,
            new_size,
            location,
        )
    }

    pub(crate) fn append_log_syscall(
        &'c self,
        block: &'c Block,
        data: Value<'c, 'c>,
        size: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        syscall::mlir::append_log_syscall(
            self.mlir_context,
            self.syscall_ctx,
            block,
            data,
            size,
            location,
        );
    }

    pub(crate) fn append_log_with_one_topic_syscall(
        &'c self,
        block: &'c Block,
        data: Value<'c, 'c>,
        size: Value<'c, 'c>,
        topic: Value<'c, 'c>,
        location: Location<'c>,
    ) {
        syscall::mlir::append_log_with_one_topic_syscall(
            self.mlir_context,
            self.syscall_ctx,
            block,
            data,
            size,
            topic,
            location,
        );
    }
}
