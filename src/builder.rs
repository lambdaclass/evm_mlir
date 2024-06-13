use crate::db::{Database, EmptyDB};

#[derive(Default)]
pub struct EvmBuilder<DB: Database> {
    db: DB,
}

impl<EmptyDB: Database> Default for EvmBuilder<EmptyDB> {
    fn default() -> Self {
        Self {
            db: EmptyDB::default(),
        }
    }
}

impl<DB: Database> EvmBuilder<DB> {
    /// Sets the [`EmptyDB`] as the [`Database`] that will be used by [`Evm`].
    pub fn with_empty_db(self) -> EvmBuilder<EmptyDB> {
        EvmBuilder {
            db: EmptyDB::default(),
        }
    }
    /// Sets the [`Database`] that will be used by [`Evm`].
    pub fn with_db<ODB: Database>(self, db: ODB) -> EvmBuilder<ODB> {
        EvmBuilder { db: ODB }
    }
}
