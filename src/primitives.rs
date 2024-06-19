pub use bytes::Bytes;
pub use ethereum_types::{Address, H160, U256};

use ethereum_types::H256;

pub type B256 = H256;

pub trait ToByteSlice {
    fn to_byte_slice(&self) -> &[u8];
}

impl ToByteSlice for H160 {
    fn to_byte_slice(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl ToByteSlice for H256 {
    fn to_byte_slice(&self) -> &[u8] {
        self.as_bytes()
    }
}
