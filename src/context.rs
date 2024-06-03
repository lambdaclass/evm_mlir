use llvm_sys::{
    core::LLVMDisposeMessage,
    target_machine::{
        LLVMCodeGenOptLevel, LLVMCodeModel, LLVMCreateTargetMachine, LLVMGetDefaultTargetTriple,
        LLVMGetHostCPUFeatures, LLVMGetHostCPUName, LLVMGetTargetFromTriple, LLVMRelocMode,
        LLVMTargetRef,
    },
};
use melior::{
    dialect::{
        arith, cf, func,
        llvm::{self, r#type::pointer, AllocaOptions, LoadStoreOptions},
        DialectRegistry,
    },
    ir::{
        attribute::{IntegerAttribute, StringAttribute, TypeAttribute},
        operation::OperationBuilder,
        r#type::{FunctionType, IntegerType},
        Block, Identifier, Location, Module as MeliorModule, Region, Value,
    },
    utility::{register_all_dialects, register_all_llvm_translations, register_all_passes},
    Context as MeliorContext,
};
use std::{
    ffi::CStr,
    mem::MaybeUninit,
    path::Path,
    ptr::{addr_of_mut, null_mut},
};

use crate::{
    codegen::{context::OperationCtx, operations::generate_code_for_op, run_pass_manager},
    constants::{MAX_STACK_SIZE, STACK_BASEPTR_GLOBAL, STACK_PTR_GLOBAL, GAS_COUNTER_GLOBAL},
    errors::CodegenError,
    module::MLIRModule,
    program::{Operation, Program},
    utils::{generate_revert_block, llvm_mlir, stack_pop},
};

#[derive(Debug, Eq, PartialEq)]
pub struct Context {
    pub melior_context: MeliorContext,
}

unsafe impl Send for Context {}
unsafe impl Sync for Context {}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    pub fn new() -> Self {
        let melior_context = initialize_mlir();
        Self { melior_context }
    }

    pub fn compile(
        &self,
        program: &Program,
        output_file: impl AsRef<Path>,
    ) -> Result<MLIRModule, CodegenError> {
        let target_triple = get_target_triple();

        let context = &self.melior_context;

        // Build a module with a single function
        let module_region = Region::new();
        let module_block = Block::new(&[]);

        module_region.append_block(module_block);

        let data_layout_ret = &get_data_layout_rep()?;

        // build main module
        let op = OperationBuilder::new("builtin.module", Location::unknown(context))
            .add_attributes(&[
                (
                    Identifier::new(context, "llvm.target_triple"),
                    StringAttribute::new(context, &target_triple).into(),
                ),
                (
                    Identifier::new(context, "llvm.data_layout"),
                    StringAttribute::new(context, data_layout_ret).into(),
                ),
            ])
            .add_regions([module_region])
            .build()?;
        assert!(op.verify(), "module operation is not valid");

        let mut melior_module = MeliorModule::from_operation(op).expect("module failed to create");

        compile_program(context, &melior_module, program)?;

        assert!(melior_module.as_operation().verify());

        let filename = output_file.as_ref().with_extension("mlir");
        std::fs::write(filename, melior_module.as_operation().to_string())?;

        // TODO: Add proper error handling.
        run_pass_manager(context, &mut melior_module)?;

        // The func to llvm pass has a bug where it sets the data layout string to ""
        // This works around it by setting it again.
        {
            let mut op = melior_module.as_operation_mut();
            op.set_attribute(
                "llvm.data_layout",
                StringAttribute::new(context, data_layout_ret).into(),
            );
        }

        // Output MLIR
        let filename = output_file.as_ref().with_extension("after-pass.mlir");
        std::fs::write(filename, melior_module.as_operation().to_string())?;

        Ok(MLIRModule::new(melior_module))
    }
}

/// Initialize an MLIR context.
pub fn initialize_mlir() -> MeliorContext {
    let context = MeliorContext::new();
    context.append_dialect_registry(&{
        let registry = DialectRegistry::new();
        register_all_dialects(&registry);
        registry
    });
    context.load_all_available_dialects();
    register_all_passes();
    register_all_llvm_translations(&context);
    context
}

pub fn get_target_triple() -> String {
    let target_triple = unsafe {
        let value = LLVMGetDefaultTargetTriple();
        CStr::from_ptr(value).to_string_lossy().into_owned()
    };
    target_triple
}

pub fn get_data_layout_rep() -> Result<String, CodegenError> {
    unsafe {
        let mut null = null_mut();
        let error_buffer = addr_of_mut!(null);

        let target_triple = LLVMGetDefaultTargetTriple();
        let target_cpu = LLVMGetHostCPUName();
        let target_cpu_features = LLVMGetHostCPUFeatures();

        let mut target: MaybeUninit<LLVMTargetRef> = MaybeUninit::uninit();

        if LLVMGetTargetFromTriple(target_triple, target.as_mut_ptr(), error_buffer) != 0 {
            let error = CStr::from_ptr(*error_buffer);
            let err = error.to_string_lossy().to_string();
            dbg!(err.clone());
            LLVMDisposeMessage(*error_buffer);
            return Err(CodegenError::LLVMCompileError(err))?;
        }
        if !(*error_buffer).is_null() {
            LLVMDisposeMessage(*error_buffer);
        }

        let target = target.assume_init();

        let machine = LLVMCreateTargetMachine(
            target,
            target_triple.cast(),
            target_cpu.cast(),
            target_cpu_features.cast(),
            LLVMCodeGenOptLevel::LLVMCodeGenLevelNone,
            LLVMRelocMode::LLVMRelocDefault,
            LLVMCodeModel::LLVMCodeModelDefault,
        );

        let data_layout = llvm_sys::target_machine::LLVMCreateTargetDataLayout(machine);
        let data_layout_str =
            CStr::from_ptr(llvm_sys::target::LLVMCopyStringRepOfTargetData(data_layout));
        Ok(data_layout_str.to_string_lossy().into_owned())
    }
}

fn compile_program(
    context: &MeliorContext,
    module: &MeliorModule,
    program: &Program,
) -> Result<(), CodegenError> {
    let location = Location::unknown(context);

    // Build a region for the main function
    let main_region = Region::new();

    // Setup the stack, memory, etc.
    // PERF: avoid generating unneeded setup blocks
    let stack_setup_block = main_region.append_block(generate_stack_setup_block(context, module)?);
    let gas_setup_block = main_region.append_block(generate_gas_counter_block(context,module)?);
    stack_setup_block.append_operation(cf::br(&gas_setup_block, &[], location));

    let revert_block = main_region.append_block(generate_revert_block(context)?);
    let jumptable_block = main_region.append_block(create_jumptable_landing_block(context));

    let mut last_block = gas_setup_block;

    let mut op_ctx = OperationCtx {
        mlir_context: context,
        program,
        revert_block,
        jumptable_block,
        jumpdest_blocks: Default::default(),
    };

    // Generate code for the program
    for op in &op_ctx.program.operations {
        let (block_start, block_end) = generate_code_for_op(&mut op_ctx, &main_region, op.clone())?;

        last_block.append_operation(cf::br(&block_start, &[], location));
        last_block = block_end;
    }

    populate_jumptable(&op_ctx)?;

    let return_block = main_region.append_block(Block::new(&[]));
    last_block.append_operation(cf::br(&return_block, &[], location));

    // Setup return operation
    // This returns the last element of the stack
    // TODO: handle case where stack is empty
    let stack_top = stack_pop(context, &return_block)?;
    // Truncate the value to 8 bits.
    // NOTE: this is due to amd64 using two registers (128 bits) for return values.
    let uint8 = IntegerType::new(context, 8);
    let exit_code = return_block
        .append_operation(arith::trunci(stack_top, uint8.into(), location))
        .result(0)?;
    return_block.append_operation(func::r#return(&[exit_code.into()], location));

    let main_func = func::func(
        context,
        StringAttribute::new(context, "main"),
        TypeAttribute::new(FunctionType::new(context, &[], &[uint8.into()]).into()),
        main_region,
        &[],
        location,
    );

    module.body().append_operation(main_func);
    Ok(())
}

fn generate_gas_counter_block<'c>(
    context: &'c MeliorContext,
    module: &'c MeliorModule,
) -> Result<Block<'c>, CodegenError> {
    let location = Location::unknown(context);
    
    let ptr_type = pointer(context, 0);

    let body = module.body();
    let res = body.append_operation(llvm_mlir::global(
        context,
        GAS_COUNTER_GLOBAL,
        ptr_type,
        location,
    ));

    let block = Block::new(&[]);
    let uint256 = IntegerType::new(context, 256);

    let gas_size = block
        .append_operation(arith::constant(
            context,
            IntegerAttribute::new(uint256.into(), 99999).into(),
            location,
        ))
        .result(0)?
        .into();

    
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
        gas_size,
        gas_addr.into(),
        location,
        LoadStoreOptions::default(),
    ));


    Ok(block)
}

fn generate_stack_setup_block<'c>(
    context: &'c MeliorContext,
    module: &'c MeliorModule,
) -> Result<Block<'c>, CodegenError> {
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

    let block = Block::new(&[]);
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

    Ok(block)
}

/// Create the jumptable landing block. This is the main entrypoint
/// for JUMP and JUMPI operations.
fn create_jumptable_landing_block(context: &MeliorContext) -> Block {
    let location = Location::unknown(context);
    let uint256 = IntegerType::new(context, 256);
    Block::new(&[(uint256.into(), location)])
}

/// Populate the jumptable block with a dynamic dispatch according to the
/// received PC.
fn populate_jumptable(op_ctx: &OperationCtx) -> Result<(), CodegenError> {
    let context = op_ctx.mlir_context;
    let program = op_ctx.program;
    let start_block = op_ctx.jumptable_block;

    let location = Location::unknown(context);
    let uint256 = IntegerType::new(context, 256);

    // The block receives a single argument: the value to switch on
    let jumpdest_pcs: Vec<i64> = program
        .operations
        .iter()
        .filter_map(|op| match op {
            Operation::Jumpdest { pc } => Some(*pc as i64),
            _ => None,
        })
        .collect();

    let arg = start_block.argument(0).unwrap();

    let case_destinations: Vec<_> = op_ctx
        .jumpdest_blocks
        .values()
        .map(|b| {
            let x: (&Block, &[Value]) = (b, &[]);
            x
        })
        .collect();

    let op = start_block.append_operation(
        cf::switch(
            context,
            &jumpdest_pcs,
            arg.into(),
            uint256.into(),
            (&op_ctx.revert_block, &[]),
            &case_destinations,
            location,
        )
        // TODO
        .unwrap(),
    );

    assert!(op.verify());

    Ok(())
}
