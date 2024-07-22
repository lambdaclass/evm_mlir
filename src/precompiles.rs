use std::array::TryFromSliceError;

use bytes::Bytes;
use secp256k1::{ecdsa, Message, Secp256k1};
use sha3::{Digest, Keccak256};

use crate::constants::precompiles::{blake2_gas_cost, ECRECOVER_COST};

pub fn ecrecover(
    calldata: &Bytes,
    gas_limit: u64,
    consumed_gas: &mut u64,
) -> Result<Bytes, secp256k1::Error> {
    if gas_limit < ECRECOVER_COST {
        return Ok(Bytes::new());
    }
    *consumed_gas += ECRECOVER_COST;
    let hash = &calldata[0..32];
    let v = calldata[63] as i32 - 27;
    let sig = &calldata[64..128];

    let msg = Message::from_digest_slice(hash)?;
    let id = ecdsa::RecoveryId::from_i32(v)?;
    let sig = ecdsa::RecoverableSignature::from_compact(sig, id)?;

    let secp = Secp256k1::new();
    let public_address = secp.recover_ecdsa(&msg, &sig)?;

    let mut hasher = Keccak256::new();
    hasher.update(&public_address.serialize_uncompressed()[1..]);
    let mut address_hash = hasher.finalize();
    address_hash[..12].fill(0);
    Ok(Bytes::copy_from_slice(&address_hash))
}

// Extracted from https://datatracker.ietf.org/doc/html/rfc7693#section-2.7
pub const SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

// Extracted from https://datatracker.ietf.org/doc/html/rfc7693#appendix-C.2
pub const IV: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

// Extracted from https://datatracker.ietf.org/doc/html/rfc7693#section-2.1
const R1: u32 = 32;
const R2: u32 = 24;
const R3: u32 = 16;
const R4: u32 = 63;

// Based on https://datatracker.ietf.org/doc/html/rfc7693#section-3.1
fn g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x); //mod 64 operations
    v[d] = (v[d] ^ v[a]).rotate_right(R1); // >>> operation
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(R2);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(R3);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(R4);
}

// Based on https://datatracker.ietf.org/doc/html/rfc7693#section-3.2
fn blake2f_compress(rounds: usize, h: &mut [u64; 8], m: &[u64; 16], t: &[u64; 2], f: bool) {
    // Initialize local work vector v[0..15]
    let mut v: [u64; 16] = [0_u64; 16];
    v[0..8].copy_from_slice(h); // First half from state
    v[8..16].copy_from_slice(&IV); // Second half from IV

    v[12] ^= t[0]; // Low word of the offset
    v[13] ^= t[1]; // High word of the offset

    if f {
        v[14] = !v[14]; // Invert all bits
    }

    for i in 0..rounds {
        // Message word selection permutation for this round
        let s: &[usize; 16] = &SIGMA[i % 10];

        g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);

        g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }

    // XOR the two halves
    for i in 0..8 {
        h[i] = h[i] ^ v[i] ^ v[i + 8];
    }
}

const CALLDATA_LEN: usize = 213;

use thiserror::Error;

#[derive(Error, Debug)]
#[error("Blake2Error")]
pub struct Blake2fError;

impl From<TryFromSliceError> for Blake2fError {
    fn from(_: TryFromSliceError) -> Self {
        Self {}
    }
}

pub fn blake2f(
    calldata: &Bytes,
    gas_limit: u64,
    consumed_gas: &mut u64,
) -> Result<Bytes, Blake2fError> {
    /*
    [0; 3] (4 bytes)	rounds	Number of rounds (big-endian unsigned integer)
    [4; 67] (64 bytes)	h	State vector (8 8-byte little-endian unsigned integer)
    [68; 195] (128 bytes)	m	Message block vector (16 8-byte little-endian unsigned integer)
    [196; 211] (16 bytes)	t	Offset counters (2 8-byte little-endian integer)
    [212; 212] (1 bytes)	f	Final block indicator flag (0 or 1)
    */

    if calldata.len() != CALLDATA_LEN {
        return Err(Blake2fError {});
    }

    let rounds = u32::from_be_bytes(calldata[0..4].try_into()?);

    let needed_gas = blake2_gas_cost(rounds);
    if needed_gas > gas_limit {
        return Err(Blake2fError {});
    }
    *consumed_gas = needed_gas;

    let mut h: [u64; 8] = [0_u64; 8];
    let mut m: [u64; 16] = [0_u64; 16];
    let mut t: [u64; 2] = [0_u64; 2];
    let f = u8::from_be_bytes(calldata[212..213].try_into()?);

    if f > 1 {
        return Err(Blake2fError {});
    }
    let f = f == 1;

    // NOTE: We may optimize this by unwraping both for loops

    for (i, h) in h.iter_mut().enumerate() {
        let start = 4 + i * 8;
        *h = u64::from_le_bytes(calldata[start..start + 8].try_into()?);
    }

    for (i, m) in m.iter_mut().enumerate() {
        let start = 68 + i * 8;
        *m = u64::from_le_bytes(calldata[start..start + 8].try_into()?);
    }

    t[0] = u64::from_le_bytes(calldata[196..204].try_into()?);
    t[1] = u64::from_le_bytes(calldata[204..212].try_into()?);

    blake2f_compress(rounds as _, &mut h, &m, &t, f);

    let out: Vec<u8> = h.iter().flat_map(|&num| num.to_le_bytes()).collect();

    Ok(Bytes::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake2_evm_codes_happy_path() {
        let rounds = hex::decode("0000000c").unwrap();
        let h = hex::decode("48c9bdf267e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5d182e6ad7f520e511f6c3e2b8c68059b6bbd41fbabd9831f79217e1319cde05b").unwrap();
        let m = hex::decode("6162630000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000").unwrap();
        let t = hex::decode("03000000000000000000000000000000").unwrap();
        let f = hex::decode("01").unwrap();
        let calldata = [rounds, h, m, t, f].concat();
        let calldata = Bytes::from(calldata);
        let gas_limit = 1000;
        let mut consumed_gas: u64 = 0;

        let expected_result = hex::decode(
        "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
    ).unwrap();
        let expected_result = Bytes::from(expected_result);
        let expected_consumed_gas = 12; //Rounds

        let result = blake2f(&calldata, gas_limit as _, &mut consumed_gas);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.len(), expected_result.len());
        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_consumed_gas);
    }

    #[test]
    fn test_blake2_eip_example_1() {
        let calldata = hex::decode("00000c48c9bdf267e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5d182e6ad7f520e511f6c3e2b8c68059b6bbd41fbabd9831f79217e1319cde05b61626300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000300000000000000000000000000000001").unwrap();
        let calldata = Bytes::from(calldata);
        let gas_limit = 1000;
        let mut consumed_gas: u64 = 0;
        let result = blake2f(&calldata, gas_limit as _, &mut consumed_gas);
        assert!(result.is_err());
    }

    #[test]
    fn test_blake2_eip_example_2() {
        let calldata = hex::decode("000000000c48c9bdf267e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5d182e6ad7f520e511f6c3e2b8c68059b6bbd41fbabd9831f79217e1319cde05b61626300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000300000000000000000000000000000001").unwrap();
        let calldata = Bytes::from(calldata);
        let gas_limit = 1000;
        let mut consumed_gas: u64 = 0;
        let result = blake2f(&calldata, gas_limit as _, &mut consumed_gas);
        assert!(result.is_err());
    }

    #[test]
    fn test_blake2_eip_example_3() {
        let calldata = hex::decode("0000000c48c9bdf267e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5d182e6ad7f520e511f6c3e2b8c68059b6bbd41fbabd9831f79217e1319cde05b61626300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000300000000000000000000000000000002").unwrap();
        let calldata = Bytes::from(calldata);
        let gas_limit = 1000;
        let mut consumed_gas: u64 = 0;
        let result = blake2f(&calldata, gas_limit as _, &mut consumed_gas);
        assert!(result.is_err());
    }

    #[test]
    fn test_blake2_eip_example_4() {
        let calldata = hex::decode("0000000048c9bdf267e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5d182e6ad7f520e511f6c3e2b8c68059b6bbd41fbabd9831f79217e1319cde05b61626300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000300000000000000000000000000000001").unwrap();
        let calldata = Bytes::from(calldata);
        let gas_limit = 1000;
        let mut consumed_gas: u64 = 0;

        let expected_result = hex::decode(
        "08c9bcf367e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5d282e6ad7f520e511f6c3e2b8c68059b9442be0454267ce079217e1319cde05b"
    ).unwrap();
        let expected_result = Bytes::from(expected_result);

        let result = blake2f(&calldata, gas_limit as _, &mut consumed_gas);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.len(), expected_result.len());
        assert_eq!(result, expected_result);
    }

    #[test]
    fn test_blake2_example_5() {
        let calldata = hex::decode("0000000c48c9bdf267e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5d182e6ad7f520e511f6c3e2b8c68059b6bbd41fbabd9831f79217e1319cde05b61626300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000300000000000000000000000000000001").unwrap();
        let calldata = Bytes::from(calldata);
        let gas_limit = 1000;
        let mut consumed_gas: u64 = 0;

        let expected_result = hex::decode(
        "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
    ).unwrap();
        let expected_result = Bytes::from(expected_result);

        let result = blake2f(&calldata, gas_limit as _, &mut consumed_gas);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.len(), expected_result.len());
        assert_eq!(result, expected_result);
    }

    #[test]
    fn test_blake2_example_6() {
        let calldata = hex::decode("0000000c48c9bdf267e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5d182e6ad7f520e511f6c3e2b8c68059b6bbd41fbabd9831f79217e1319cde05b61626300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000300000000000000000000000000000000").unwrap();
        let calldata = Bytes::from(calldata);
        let gas_limit = 1000;
        let mut consumed_gas: u64 = 0;

        let expected_result = hex::decode(
        "75ab69d3190a562c51aef8d88f1c2775876944407270c42c9844252c26d2875298743e7f6d5ea2f2d3e8d226039cd31b4e426ac4f2d3d666a610c2116fde4735"
    ).unwrap();
        let expected_result = Bytes::from(expected_result);

        let result = blake2f(&calldata, gas_limit as _, &mut consumed_gas);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.len(), expected_result.len());
        assert_eq!(result, expected_result);
    }

    #[test]
    fn test_blake2_example_7() {
        let calldata = hex::decode("0000000148c9bdf267e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5d182e6ad7f520e511f6c3e2b8c68059b6bbd41fbabd9831f79217e1319cde05b61626300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000300000000000000000000000000000001").unwrap();
        let calldata = Bytes::from(calldata);
        let gas_limit = 1000;
        let mut consumed_gas: u64 = 0;

        let expected_result = hex::decode(
        "b63a380cb2897d521994a85234ee2c181b5f844d2c624c002677e9703449d2fba551b3a8333bcdf5f2f7e08993d53923de3d64fcc68c034e717b9293fed7a421"
    ).unwrap();
        let expected_result = Bytes::from(expected_result);
        let expected_consumed_gas = 1;

        let result = blake2f(&calldata, gas_limit as _, &mut consumed_gas);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.len(), expected_result.len());
        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_consumed_gas);
    }
}
