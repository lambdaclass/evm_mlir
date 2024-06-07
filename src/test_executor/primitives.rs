use core::str::FromStr;
use serde::{Deserialize, Deserializer};

const ADDRESS_SIZE: usize = 20;

use std::fmt;
use std::num::ParseIntError;

pub use bytes::Bytes;

/*
TODO:

An Ethereum address has two representations.
The first one doesn't have a validation and it has been present since the beginning.
The second one implemented a checksum validation after EIP-55, so it has some differences.

Both of those representations can be used, but only the second one can be validated making use of checksum.

For the moment, this implementation will not use any checksum, but this should change in the short future.
We should also add some unit tests.
*/

/*
TODO:
Improve error handling
*/

#[derive(PartialEq, Eq, Clone, Hash)]
pub struct Address {
    bytes: [u8; ADDRESS_SIZE],
}

impl Address {
    /// Creates an Address from a Big Endian bytes slice
    /// The received slice must have a size equal to 20 bytes, which is
    /// the size of an Ethereum Address.
    pub fn from_bytes_exact(data: &[u8]) -> Address {
        if data.len() != ADDRESS_SIZE {
            panic!("Addresses are 20 bytes long");
        }
        let mut bytes = [0_u8; ADDRESS_SIZE];
        bytes.copy_from_slice(data);

        Address { bytes }
    }

    /// Creates an Address from a String slice.
    /// The received string:
    ///     - Must represent the bytes's address in hex format.
    ///     - Must be of size 42 if it starts with "0x".
    ///     - Must be of size 40 if it doesn't start with "0x".
    pub fn from_string_exact(data: &str) -> Address {
        match data.len() {
            40 => {
                let bytes = Self::decode_hex(data).unwrap();
                Address::from_bytes_exact(&bytes)
            }
            42 => {
                if !data.starts_with("0x") {
                    panic!("Addresses are 20 bytes long");
                }
                Self::from_string_exact(&data[2..])
            }
            _ => {
                panic!("Addresses are 20 bytes long");
            }
        }
    }

    fn decode_hex(s: &str) -> Result<Vec<u8>, ParseIntError> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
            .collect()
    }

    fn bytes_to_hex(bytes: &[u8]) -> String {
        bytes
            .iter()
            .fold(String::new(), |acc, byte| format!("{}{:02x}", acc, byte))
    }
}

impl From<String> for Address {
    #[inline]
    fn from(value: String) -> Self {
        Address::from_string_exact(&value)
    }
}

impl From<&[u8]> for Address {
    #[inline]
    fn from(value: &[u8]) -> Self {
        Address::from_bytes_exact(value)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let output = Self::bytes_to_hex(&self.bytes);
        write!(f, "0x{output}")
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let output = Self::bytes_to_hex(&self.bytes);
        write!(f, "0x{output}")
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Address, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        Ok(Address::from_string_exact(&string))
    }
}

impl FromStr for Address {
    type Err = u32;
    fn from_str(s: &str) -> Result<Address, Self::Err> {
        Ok(Address::from_string_exact(s))
    }
}
