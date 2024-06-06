use llvm_sys::{
    core::LLVMDisposeMessage,
    target::{
        LLVM_InitializeAllAsmPrinters, LLVM_InitializeAllTargetInfos, LLVM_InitializeAllTargetMCs,
        LLVM_InitializeAllTargets,
    },
    target_machine::{
        LLVMCodeGenOptLevel, LLVMCodeModel, LLVMCreateTargetMachine, LLVMGetDefaultTargetTriple,
        LLVMGetHostCPUFeatures, LLVMGetHostCPUName, LLVMGetTargetFromTriple, LLVMRelocMode,
        LLVMTargetRef,
    },
};
use melior::{
    dialect::{arith, cf, func, llvm::r#type::pointer, DialectRegistry},
    ir::{
        attribute::{StringAttribute, TypeAttribute},
        operation::OperationBuilder,
        r#type::{FunctionType, IntegerType},
        Attribute, Block, Identifier, Location, Module as MeliorModule, Region,
    },
    utility::{register_all_dialects, register_all_llvm_translations, register_all_passes},
    Context as MeliorContext,
};
use std::{
    ffi::CStr,
    mem::MaybeUninit,
    path::Path,
    ptr::{addr_of_mut, null_mut},
    sync::OnceLock,
};

use crate::{
    codegen::{context::OperationCtx, operations::generate_code_for_op, run_pass_manager},
    constants::MAIN_ENTRYPOINT,
    errors::CodegenError,
    module::MLIRModule,
    program::Program,
    utils::stack_pop,
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
        static INITIALIZED: OnceLock<()> = OnceLock::new();
        INITIALIZED.get_or_init(|| unsafe {
            LLVM_InitializeAllTargets();
            LLVM_InitializeAllTargetInfos();
            LLVM_InitializeAllTargetMCs();
            LLVM_InitializeAllAsmPrinters();
        });

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
    let ptr_type = pointer(context, 0);
    let uint8 = IntegerType::new(context, 8).into();
    let uint64 = IntegerType::new(context, 64).into();

    // Build the main function
    let main_func = func::func(
        context,
        StringAttribute::new(context, MAIN_ENTRYPOINT),
        TypeAttribute::new(FunctionType::new(context, &[ptr_type, uint64], &[uint8]).into()),
        Region::new(),
        &[
            (
                Identifier::new(context, "sym_visibility"),
                StringAttribute::new(context, "public").into(),
            ),
            (
                Identifier::new(context, "llvm.emit_c_interface"),
                Attribute::unit(context),
            ),
        ],
        location,
    );

    let main_region = main_func.region(0).unwrap();

    // Setup the stack, memory, etc.
    // PERF: avoid generating unneeded setup blocks
    let setup_block = main_region.append_block(Block::new(&[]));

    let mut op_ctx = OperationCtx::new(context, module, &main_region, &setup_block, program)?;

    let mut last_block = setup_block;

    // Generate code for the program
    for op in &op_ctx.program.operations {
        let (block_start, block_end) = generate_code_for_op(&mut op_ctx, &main_region, op.clone())?;

        last_block.append_operation(cf::br(&block_start, &[], location));
        last_block = block_end;
    }

    op_ctx.populate_jumptable()?;

    let return_block = main_region.append_block(Block::new(&[]));
    last_block.append_operation(cf::br(&return_block, &[], location));

    // Setup return operation
    // This returns the last element of the stack
    // TODO: this should return nothing
    let stack_top = stack_pop(context, &return_block)?;
    // Truncate the value to 8 bits.
    // NOTE: this is due to amd64 using two registers (128 bits) for return values.
    let exit_code = return_block
        .append_operation(arith::trunci(stack_top, uint8, location))
        .result(0)?
        .into();
    return_block.append_operation(func::r#return(&[exit_code], location));

    module.body().append_operation(main_func);
    Ok(())
}
