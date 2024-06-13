use ethereum_types::Address;

#[derive(Clone, Debug, Default)]
pub struct Env {
    /// Block-related info
    pub block: BlockEnv,
    /// Transaction-related info
    pub tx: TxEnv,
}

#[derive(Clone, Debug, Default)]
pub struct BlockEnv {
    pub number: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TxEnv {
    pub from: Address,
    pub to: Address,
    pub calldata: Vec<u8>,
    pub gas_limit: u64,
}
