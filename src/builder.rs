use crate::db::Database;

#[derive(Default)]
#[allow(dead_code)] //TODO: remove
pub struct EvmBuilder<DB: Database> {
    db: DB,
}

impl<DB: Database + Default> EvmBuilder<DB> {
    /// Sets the [`Database`] that will be used by [`Evm`].
    pub fn with_db(self, db: DB) -> EvmBuilder<DB> {
        EvmBuilder { db }
    }
}
