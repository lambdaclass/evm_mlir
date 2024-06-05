#[derive(Clone, Debug, Default)]
pub struct Env {
    pub block: BlockEnv,
    pub tx: TxEnv,
}

#[derive(Clone, Debug, Default)]
pub struct BlockEnv {}

#[derive(Clone, Debug, Default)]
pub struct TxEnv {
    #[allow(unused)]
    pub calldata: Vec<u8>,
    #[allow(unused)]
    pub gas_limit: u64,
}
