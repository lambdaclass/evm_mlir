use builder::EvmBuilder;
use db::{Database, Db};
use env::TransactTo;
use executor::{Executor, OptLevel};
use journal::Journal;
use num_bigint::BigUint;
use program::{Operation, Program};
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
    fn call_address(&mut self) -> Result<ResultAndState, EVMError> {
        let context = Context::new();
        let code_address = self.env.tx.get_address();
        //TODO: Improve error handling
        let bytecode = self
            .db
            .code_by_address(code_address)
            .expect("Failed to get code from address");
        let program = Program::from_bytecode(&bytecode);

        self.env.consume_intrinsic_cost()?;
        self.env.validate_transaction()?;
        // validate transaction

        let module = context
            .compile(&program, Default::default())
            .expect("failed to compile program");

        let call_frame = CallFrame::new(self.env.tx.caller);
        let journal = Journal::new(&mut self.db);
        let mut context = SyscallContext::new(self.env.clone(), journal, call_frame);
        let executor = Executor::new(&module, &context, OptLevel::Aggressive);

        // TODO: improve this once we stabilize the API a bit
        context.inner_context.program = program.to_bytecode();
        executor.execute(&mut context, self.env.tx.gas_limit);

        context.get_result()
    }

    fn create(&mut self) -> Result<ResultAndState, EVMError> {
        let context = Context::new();
        let initialization_code = self.env.tx.data.clone().to_vec();
        let mut starting_code: Vec<Operation> = Vec::new();
        let mut total_size = 0;
        for chunk in initialization_code.chunks(32) {
            let mut init_code = vec![
                Operation::Push((chunk.len() as u8, BigUint::from_bytes_be(chunk))),
                Operation::Push((1, BigUint::ZERO)),
                Operation::Mstore,
            ];
            total_size += chunk.len();
            starting_code.append(&mut init_code);
        }

        let operations = [
            starting_code,
            vec![
                Operation::Push((1, BigUint::from(total_size))),
                Operation::Push((1, BigUint::ZERO)),
                Operation::Push((1, BigUint::ZERO)),
                Operation::Create,
            ],
        ]
        .concat();

        let program = Program::from(operations);

        self.env.consume_intrinsic_cost()?;
        self.env.validate_transaction()?;
        // validate transaction

        let module = context
            .compile(&program, Default::default())
            .expect("failed to compile program");

        let call_frame = CallFrame::new(self.env.tx.caller);
        let journal = Journal::new(&mut self.db);
        let mut context = SyscallContext::new(self.env.clone(), journal, call_frame);
        let executor = Executor::new(&module, &context, OptLevel::Aggressive);

        // TODO: improve this once we stabilize the API a bit
        context.inner_context.program = program.to_bytecode();
        executor.execute(&mut context, self.env.tx.gas_limit);

        context.get_result()
    }

    /// Executes [the configured transaction](Env::tx).
    pub fn transact(&mut self) -> Result<ResultAndState, EVMError> {
        match self.env.tx.transact_to {
            TransactTo::Call(_) => self.call_address(),
            TransactTo::Create => self.create(),
        }
    }

    pub fn transact_commit(&mut self) -> Result<ExecutionResult, EVMError> {
        let ResultAndState { state, result } = self.transact()?;
        self.db.commit(state);
        Ok(result)
    }
}
