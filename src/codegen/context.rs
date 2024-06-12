use std::collections::BTreeMap;

use melior::{
    dialect::{
        arith, cf, func,
        llvm::{self, r#type::pointer, AllocaOptions, LoadStoreOptions},
        ods::affine::vector_load,
    },
    ir::{
        attribute::{ArrayAttribute, Attribute, IntegerAttribute, TypeAttribute},
        r#type::IntegerType,
        Block, BlockRef, Location, Module, Region, Value,
    },
    Context as MeliorContext,
};
use num_bigint::BigUint;

use crate::{
    constants::{
        CODE_PTR_GLOBAL, GAS_COUNTER_GLOBAL, MAX_STACK_SIZE, MEMORY_PTR_GLOBAL, MEMORY_SIZE_GLOBAL,
        STACK_BASEPTR_GLOBAL, STACK_PTR_GLOBAL,
    },
    errors::CodegenError,
    program::{self, Operation, Program},
    syscall::{self, ExitStatusCode},
    utils::{get_remaining_gas, integer_constant_from_i64, integer_constant_from_u8, llvm_mlir},
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
        generate_program_code_setup_code(context, module, setup_block, program)?;

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

fn generate_program_code_setup_code<'c>(
    context: &'c MeliorContext,
    module: &'c Module,
    block: &'c Block<'c>,
    program: &'c Program,
) -> Result<(), CodegenError> {
    let location = Location::unknown(context);
    let ptr_type = pointer(context, 0);
    let uint256 = IntegerType::new(context, 256);
    let uint8 = IntegerType::new(context, 8);

    let body = module.body();
    // declare a global CODE_PTR_GLOBAL
    let res = body.append_operation(llvm_mlir::global(
        context,
        CODE_PTR_GLOBAL,
        ptr_type,
        location,
    ));
    assert!(res.verify());

    let code_size = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint256.into(), program.code_size as i64).into(),
            location,
        ))
        .result(0)?
        .into();

    // allocate memory for the code
    let code_ptr = block
        .append_operation(llvm::alloca(
            context,
            code_size,
            ptr_type,
            location,
            AllocaOptions::new().elem_type(TypeAttribute::new(uint8.into()).into()),
        ))
        .result(0)?
        .into();

    let codeptr_ptr = block
        .append_operation(llvm_mlir::addressof(
            context,
            CODE_PTR_GLOBAL,
            ptr_type,
            location,
        ))
        .result(0)?
        .into();

    // store the address of code_ptr (where the code is) in CODE_PTR_GLOBAL
    let res = block.append_operation(llvm::store(
        context,
        code_ptr,
        codeptr_ptr,
        location,
        LoadStoreOptions::default(),
    ));
    assert!(res.verify());

    // here the program is converted from a Vec<Operation> to a ArrayAttribute constant
    let operations = program.operations.clone();

    let mut byte_code = vec![];
    for operation in operations {
        let opcode = operation.to_bytecode();
        for byte in opcode {
            byte_code.push(byte);
        }
    }
    let mut code_as_attribute: Vec<Attribute> = vec![];
    for byte in byte_code {
        let byte_attribute = IntegerAttribute::new(uint8.into(), byte as i64);
        code_as_attribute.push(byte_attribute.into());
    }

    let mut index = 0;
    // store the code in memory byte by byte
    for atr in code_as_attribute {
        let constant: Value = block
            .append_operation(arith::constant(context, atr, location))
            .result(0)?
            .into();
        let constant_index = block
            .append_operation(arith::constant(
                context,
                integer_constant_from_i64(context, index as i64).into(),
                location,
            ))
            .result(0)?
            .into();
        let ptr: Value = block
            .append_operation(llvm::get_element_ptr_dynamic(
                context,
                code_ptr,
                &[constant_index],
                uint8.into(),
                ptr_type,
                location,
            ))
            .result(0)?
            .into();
        let res = block.append_operation(llvm::store(
            context,
            constant,
            ptr,
            location,
            LoadStoreOptions::new()
                .align(IntegerAttribute::new(IntegerType::new(context, 64).into(), 1).into()),
        ));
        assert!(res.verify());
        index += 1;
    }

    // store the code in memory
    /*let res = block.append_operation(llvm::store(
        context,
        code_constant,
        code_ptr,
        location,
        LoadStoreOptions::default(),
    ));
    */
    //assert!(res.verify());

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

    pub(crate) fn get_calldata_size_syscall(
        &'c self,
        block: &'c Block,
        location: Location<'c>,
    ) -> Result<Value, CodegenError> {
        syscall::mlir::get_calldata_size_syscall(
            self.mlir_context,
            self.syscall_ctx,
            block,
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
}
