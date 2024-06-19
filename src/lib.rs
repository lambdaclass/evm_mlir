use std::path::PathBuf;

use builder::EvmBuilder;
use db::{Database, Db};
use env::TransactTo;
use executor::{Executor, OptLevel};
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
pub mod primitives;
pub mod program;
pub mod syscall;
pub mod utils;
pub use env::Env;

#[derive(Debug)]
pub struct Evm<DB: Database> {
    pub env: Env,
    pub db: DB,
}

impl<DB: Database + Default> Evm<DB> {
    /// Returns evm builder with empty database.
    pub fn builder() -> EvmBuilder<DB> {
        EvmBuilder::default()
    }

    /// Creates a new EVM instance with the given environment and database.
    pub fn new(env: Env, db: DB) -> Self {
        Self { env, db }
    }
}

impl Evm<Db> {
    /// Executes [the configured transaction](Env::tx).
    pub fn transact(&mut self) -> ExecutionResult {
        let output_file = PathBuf::from("output");

        let context = Context::new();

        let code_address = match self.env.tx.transact_to {
            TransactTo::Call(code_address) => code_address,
            TransactTo::Create => unimplemented!(), // TODO: implement creation
        };
        let bytecode = self
            .db
            .code_by_address(code_address)
            .expect("failed to load bytecode");
        let program = Program::from_bytecode(&bytecode).unwrap(); // TODO: map invalid/unknown opcodes to INVALID operation

        let module = context
            .compile(&program, &output_file)
            .expect("failed to compile program");

        let executor = Executor::new(&module, OptLevel::Aggressive);
        let mut context = SyscallContext::new(self.env.clone(), &mut self.db);

        // TODO: improve this once we stabilize the API a bit
        context.inner_context.program = program.to_bytecode();
        executor.execute(&mut context, self.env.tx.gas_limit);

        context.get_result()
    }
}
