use builder::EvmBuilder;
use db::{Database, DatabaseError, Db};
use env::TransactTo;
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
    fn validate_transaction(&mut self) -> Result<(), EVMError> {
        self.env.consume_intrinsic_cost()?;
        self.env.validate_transaction()?;

        Ok(())
    }

    fn create_syscall_context(&mut self) -> SyscallContext {
        let call_frame = CallFrame::new(self.env.tx.caller);
        let journal = Journal::new(&mut self.db);
        SyscallContext::new(self.env.clone(), journal, call_frame)
    }

    fn run_program(&mut self, program: Program) -> Result<ResultAndState, EVMError> {
        let context = Context::new();
        let module = context
            .compile(&program, Default::default())
            .expect("failed to compile program");

        let initial_gas = self.env.tx.gas_limit;
        let mut context = self.create_syscall_context();
        let executor = Executor::new(&module, &context, OptLevel::Aggressive);

        // TODO: improve this once we stabilize the API a bit
        context.inner_context.program = program.to_bytecode();
        executor.execute(&mut context, initial_gas);

        context.get_result()
    }

    fn call(&mut self) -> Result<ResultAndState, EVMError> {
        self.validate_transaction()?;
        let code_address = self.env.tx.get_address();

        let bytecode = match self.db.code_by_address(code_address) {
            Ok(bytecode) => bytecode,
            Err(_) => return Err(EVMError::Database(DatabaseError)),
        };

        let program = Program::from_bytecode(&bytecode);
        self.run_program(program)
    }

    fn get_env_value(&self) -> syscall::U256 {
        let mut ethereum_value = self.env.tx.value.0.to_vec();
        ethereum_value.reverse(); // we have to reverse the bytes, it's in little endian and we use big endian with syscall
        let mut value = [0u8; 32];

        for (i, num) in ethereum_value.iter().enumerate() {
            value[i * 8..(i + 1) * 8].copy_from_slice(&num.to_be_bytes());
        }

        syscall::U256::from_fixed_be_bytes(value)
    }

    fn create(&mut self) -> Result<ResultAndState, EVMError> {
        self.validate_transaction()?;

        let mut value = self.get_env_value();
        let mut gas_limit = self.env.tx.gas_limit;
        let program = self.env.tx.data.to_vec();
        let program_size = program.len() as u32;

        let mut context = self.create_syscall_context();
        context.inner_context.memory = program;

        context.create(program_size, 0, &mut value, &mut gas_limit);
        context.get_result()
    }

    /// Executes [the configured transaction](Env::tx).
    pub fn transact(&mut self) -> Result<ResultAndState, EVMError> {
        match self.env.tx.transact_to {
            TransactTo::Call(_) => self.call(),
            TransactTo::Create => self.create(),
        }
    }

    pub fn transact_commit(&mut self) -> Result<ExecutionResult, EVMError> {
        let ResultAndState { state, result } = self.transact()?;
        self.db.commit(state);
        Ok(result)
    }
}
