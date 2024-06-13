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

impl TxEnv {
    pub fn get_address(&self) -> Vec<u8> {
        let address = self.to;
        let mut bytes: [u8; 20] = Default::default();
        bytes.copy_from_slice(&address[0..20]);
        bytes.into()
    }
}
