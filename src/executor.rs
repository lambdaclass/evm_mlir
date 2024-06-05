use melior::ExecutionEngine;

use crate::{
    constants::MAIN_ENTRYPOINT,
    module::MLIRModule,
    syscall::{self, MainFunc, SyscallContext},
};

pub struct Executor {
    engine: ExecutionEngine,
}

impl Executor {
    pub fn new(module: &MLIRModule) -> Self {
        let engine = ExecutionEngine::new(module.module(), 0, &[], false);
        syscall::register_syscalls(&engine);
        Self { engine }
    }

    pub fn execute(&self, context: &mut SyscallContext, initial_gas: u64) -> u8 {
        let main_fn: MainFunc = self.get_main_entrypoint();

        main_fn(context, initial_gas)
    }

    fn get_main_entrypoint(&self) -> MainFunc {
        let function_name = format!("_mlir_ciface_{MAIN_ENTRYPOINT}");
        let fptr = self.engine.lookup(&function_name);
        unsafe { std::mem::transmute(fptr) }
    }
}
