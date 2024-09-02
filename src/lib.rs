use builder::EvmBuilder;
use db::{Database, Db};
use executor::{Executor, OptLevel};
use journal::Journal;
use program::Program;
use result::{EVMError, ExecutionResult, ResultAndState};
use syscall::{CallFrame, SyscallContext};

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
pub mod journal;
pub mod precompiles;
pub mod result;
pub mod state;

#[derive(Debug)]
pub struct Evm<DB: Database> {
    pub env: Box<Env>,
    pub db: Box<DB>,
}

impl<DB: Database + Default> Evm<DB> {
    /// Returns evm builder with empty database.
    pub fn builder() -> EvmBuilder<DB> {
        EvmBuilder::default()
    }

    /// Creates a new EVM instance with the given environment and database.
    pub fn new(env: Env, db: DB) -> Self {
        Self {
            env: Box::new(env),
            db: Box::new(db),
        }
    }
}

impl Evm<Db> {
    /// Executes [the configured transaction](Env::tx).
    pub fn transact(&mut self) -> Result<ResultAndState, EVMError> {
        let context = Box::new(Context::new());
        let code_address = self.env.tx.get_address();

        //TODO: Improve error handling
        let bytecode = Box::new(
            self.db
                .code_by_address(code_address)
                .expect("Failed to get code from address"),
        );
        let program = Box::new(Program::from_bytecode(&bytecode));

        self.env.consume_intrinsic_cost()?;
        self.env.validate_transaction()?;
        // validate transaction

        let module = Box::new(
            context
                .compile(&program, Default::default())
                .expect("failed to compile program"),
        );

        let call_frame = CallFrame::new(self.env.tx.caller);

        let journal = Journal::new(&mut self.db);
        let mut context = Box::new(SyscallContext::new(self.env.clone(), journal, call_frame));
        // aca pincha
        let executor = Box::new(Executor::new(&module, &context, OptLevel::Aggressive));

        // TODO: improve this once we stabilize the API a bit
        context.inner_context.program = program.to_bytecode();
        executor.execute(&mut context, self.env.tx.gas_limit);

        context.get_result()
    }

    pub fn transact_commit(&mut self) -> Result<ExecutionResult, EVMError> {
        let ResultAndState { state, result } = self.transact()?;
        self.db.commit(state);
        Ok(result)
    }
}
