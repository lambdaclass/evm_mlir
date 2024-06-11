use std::path::PathBuf;

use executor::Executor;
use program::Program;
use syscall::{ExecutionResult, SyscallContext};

use crate::context::Context;

pub mod codegen;
pub mod constants;
pub mod context;
pub mod env;
pub mod errors;
pub mod executor;
pub mod module;
pub mod program;
pub mod syscall;
pub mod utils;

pub use env::Env;

#[derive(Debug)]
pub struct Evm {
    pub env: Env,
    pub program: Program,
}

impl Evm {
    /// Creates a new EVM instance with the given environment and program.
    // TODO: the program should be loaded from the bytecode of the configured transaction.
    pub fn new(env: Env, program: Program) -> Self {
        Self { env, program }
    }

    /// Executes [the configured transaction](Env::tx).
    pub fn transact(&self) -> ExecutionResult {
        let output_file = PathBuf::from("output");

        let context = Context::new();
        let module = context
            .compile(&self.program, &output_file)
            .expect("failed to compile program");

        let executor = Executor::new(&module);
        let mut context = SyscallContext::with_env(self.env.clone());

        executor.execute(&mut context, self.env.tx.gas_limit);
        context.get_result()
    }
}
