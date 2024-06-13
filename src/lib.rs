use std::path::PathBuf;

use builder::EvmBuilder;
use db::{Database, Db, EmptyDb};
use executor::Executor;
use program::Program;
use syscall::{ExecutionResult, SyscallContext};

use crate::context::Context;

pub mod builder;
pub mod codegen;
pub mod constants;
pub mod context;
pub mod db;
pub mod env;
pub mod errors;
pub mod executor;
pub mod module;
pub mod program;
pub mod syscall;
pub mod utils;
pub use env::Env;

#[derive(Debug)]
pub struct Evm<DB: Database> {
    pub env: Env,
    pub program: Program,
    pub db: DB,
}
// mantener el mismo new de antes, copiar el builder y todo lo que haga falta (que sea publico de ahi). Los generics no hace falta.s
// implementar una emptyDb

impl<'a> Evm<EmptyDB> {
    /// Returns evm builder with empty database and empty external context.
    pub fn builder() -> EvmBuilder<EmptyDB> {
        EvmBuilder::default()
    }
}

impl Evm {
    /// Creates a new EVM instance with the given environment and program.
    // TODO: the program should be loaded from the bytecode of the configured transaction.
    pub fn new(env: Env, program: Program) -> Self {
        let db = Db::default();

        Self { env, program, db }
    }

    /// Executes [the configured transaction](Env::tx).
    pub fn transact(&mut self) -> ExecutionResult {
        let output_file = PathBuf::from("output");

        let context = Context::new();
        let module = context
            .compile(&self.program, &output_file)
            .expect("failed to compile program");

        let executor = Executor::new(&module);
        let mut context = SyscallContext::new(self.env.clone(), &mut self.db);

        executor.execute(&mut context, self.env.tx.gas_limit);
        context.get_result()
    }
}
