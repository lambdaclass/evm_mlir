use crate::syscall::U256;
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
    pub fn get_address(&self) -> U256 {
        // address is 20-bytes long
        // we put the 16 less-significative bytes of address in lo_bytes
        // and the 4 most-significative bytes in hi_bytes, and return
        // both of these as a U256
        let address = self.to;
        let mut lo_bytes: [u8; 16] = Default::default();
        lo_bytes.copy_from_slice(&address[4..20]);
        let lo = u128::from_be_bytes(lo_bytes);

        let mut hi_bytes: [u8; 16] = [0; 16];
        hi_bytes[15] = address[3];
        hi_bytes[14] = address[2];
        hi_bytes[13] = address[1];
        hi_bytes[12] = address[0];
        let hi = u128::from_be_bytes(hi_bytes);

        U256 { lo, hi }
    }
}
