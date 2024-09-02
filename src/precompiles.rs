use crate::{
    constants::precompiles::{
        blake2_gas_cost, identity_dynamic_cost, ripemd_160_dynamic_cost, sha2_256_dynamic_cost,
        ECADD_COST, ECMUL_COST, ECPAIRING_COST, ECRECOVER_COST, IDENTITY_COST, RIPEMD_160_COST,
        SHA2_256_COST,
    },
    primitives::U256,
};
use bytes::Bytes;
use lambdaworks_math::{
    cyclic_group::IsGroup,
    elliptic_curve::{
        short_weierstrass::curves::bn_254::{
            curve::{BN254Curve, BN254FieldElement, BN254TwistCurveFieldElement},
            field_extension::Degree12ExtensionField,
            pairing::BN254AtePairing,
            twist::BN254TwistCurve,
        },
        traits::{IsEllipticCurve, IsPairing},
    },
    field::{element::FieldElement, extensions::quadratic::QuadraticExtensionFieldElement},
    traits::ByteConversion,
    unsigned_integer::element::U256 as LambdaWorksU256,
};
use num_bigint::BigUint;
use secp256k1::{ecdsa, Message, Secp256k1};
use sha3::{Digest, Keccak256};
use std::array::TryFromSliceError;

pub fn ecrecover(
    calldata: &Bytes,
    gas_limit: u64,
    consumed_gas: &mut u64,
) -> Result<Bytes, secp256k1::Error> {
    if gas_limit < ECRECOVER_COST || calldata.len() < 128 {
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

pub fn identity(calldata: &Bytes, gas_limit: u64, consumed_gas: &mut u64) -> Bytes {
    let gas_cost = IDENTITY_COST + identity_dynamic_cost(calldata.len() as u64);
    if gas_limit < gas_cost {
        return Bytes::new();
    }
    *consumed_gas += gas_cost;
    calldata.clone()
}

pub fn sha2_256(calldata: &Bytes, gas_limit: u64, consumed_gas: &mut u64) -> Bytes {
    let gas_cost = SHA2_256_COST + sha2_256_dynamic_cost(calldata.len() as u64);
    if gas_limit < gas_cost {
        return Bytes::new();
    }
    *consumed_gas += gas_cost;
    let hash = sha2::Sha256::digest(calldata);
    Bytes::copy_from_slice(&hash)
}

pub fn ripemd_160(calldata: &Bytes, gas_limit: u64, consumed_gas: &mut u64) -> Bytes {
    let gas_cost = RIPEMD_160_COST + ripemd_160_dynamic_cost(calldata.len() as u64);
    if gas_limit < gas_cost {
        return Bytes::new();
    }
    *consumed_gas += gas_cost;
    let mut hasher = ripemd::Ripemd160::new();
    hasher.update(calldata);
    let mut output = [0u8; 32];
    hasher.finalize_into((&mut output[12..]).into());
    Bytes::copy_from_slice(&output)
}

pub fn modexp(calldata: &Bytes, gas_limit: u64, consumed_gas: &mut u64) -> Bytes {
    if calldata.len() < 96 {
        return Bytes::new();
    }

    // Cast sizes as usize and check for overflow.
    // Bigger sizes are not accepted, as memory can't index bigger values.
    let Ok(b_size) = usize::try_from(U256::from_big_endian(&calldata[0..32])) else {
        return Bytes::new();
    };
    let Ok(e_size) = usize::try_from(U256::from_big_endian(&calldata[32..64])) else {
        return Bytes::new();
    };
    let Ok(m_size) = usize::try_from(U256::from_big_endian(&calldata[64..96])) else {
        return Bytes::new();
    };

    // Check if calldata contains all values
    let params_len = 96 + b_size + e_size + m_size;
    if calldata.len() < params_len {
        return Bytes::new();
    }
    let b = BigUint::from_bytes_be(&calldata[96..96 + b_size]);
    let e = BigUint::from_bytes_be(&calldata[96 + b_size..96 + b_size + e_size]);
    let m = BigUint::from_bytes_be(&calldata[96 + b_size + e_size..params_len]);

    // Compute gas cost
    let max_length = b_size.max(m_size);
    let words = (max_length + 7) / 8;
    let multiplication_complexity = (words * words) as u64;
    let iteration_count = if e_size <= 32 && e != BigUint::ZERO {
        e.bits() - 1
    } else if e_size > 32 {
        8 * (e_size as u64 - 32) + e.bits().max(1) - 1
    } else {
        0
    };
    let calculate_iteration_count = iteration_count.max(1);
    let gas_cost = (multiplication_complexity * calculate_iteration_count / 3).max(200);
    if gas_limit < gas_cost {
        return Bytes::new();
    }
    *consumed_gas += gas_cost;

    let result = if m == BigUint::ZERO {
        BigUint::ZERO
    } else if e == BigUint::ZERO {
        BigUint::from(1_u8) % m
    } else {
        b.modpow(&e, &m)
    };

    let output = &result.to_bytes_be()[..m_size];
    Bytes::copy_from_slice(output)
}

pub fn ecadd(calldata: &Bytes, gas_limit: u64, consumed_gas: &mut u64) -> Bytes {
    if calldata.len() < 128 || gas_limit < ECADD_COST {
        *consumed_gas += gas_limit;
        return Bytes::new();
    }
    *consumed_gas += ECADD_COST;

    // Slice lengths are checked, so unwrap is safe
    let x1 = BN254FieldElement::from_bytes_be(&calldata[..32]).unwrap();
    let y1 = BN254FieldElement::from_bytes_be(&calldata[32..64]).unwrap();
    let x2 = BN254FieldElement::from_bytes_be(&calldata[64..96]).unwrap();
    let y2 = BN254FieldElement::from_bytes_be(&calldata[96..128]).unwrap();

    // (0,0) represents infinity, in that case the other point (if valid) should be directly returned
    let zero_el = BN254FieldElement::from(0);
    let p1_is_infinity = x1.eq(&zero_el) && y1.eq(&zero_el);
    let p2_is_infinity = x2.eq(&zero_el) && y2.eq(&zero_el);

    match (p1_is_infinity, p2_is_infinity) {
        (true, true) => return Bytes::from([0u8; 64].to_vec()),
        (true, false) => {
            if let Ok(p2) = BN254Curve::create_point_from_affine(x2, y2) {
                let res = [p2.x().to_bytes_be(), p2.y().to_bytes_be()].concat();
                return Bytes::from(res);
            }
            return Bytes::new();
        }
        (false, true) => {
            if let Ok(p1) = BN254Curve::create_point_from_affine(x1, y1) {
                let res = [p1.x().to_bytes_be(), p1.y().to_bytes_be()].concat();
                return Bytes::from(res);
            }
            return Bytes::new();
        }
        _ => {}
    }

    if let Ok(p1) = BN254Curve::create_point_from_affine(x1, y1) {
        if let Ok(p2) = BN254Curve::create_point_from_affine(x2, y2) {
            let sum = p1.operate_with(&p2).to_affine();
            let res = [sum.x().to_bytes_be(), sum.y().to_bytes_be()].concat();
            return Bytes::from(res);
        }
    }
    Bytes::new()
}

pub fn ecmul(calldata: &Bytes, gas_limit: u64, consumed_gas: &mut u64) -> Bytes {
    if calldata.len() < 96 || gas_limit < ECMUL_COST {
        *consumed_gas += gas_limit;
        return Bytes::new();
    }
    *consumed_gas += ECMUL_COST;

    // Slice lengths are checked, so unwrap is safe
    let x1 = BN254FieldElement::from_bytes_be(&calldata[..32]).unwrap();
    let y1 = BN254FieldElement::from_bytes_be(&calldata[32..64]).unwrap();
    let s = LambdaWorksU256::from_bytes_be(&calldata[64..96]).unwrap();

    // if the point is infinity it is directly returned
    let zero_el = BN254FieldElement::from(0);
    let zero_u256 = LambdaWorksU256::from(0_u16);
    let p1_is_infinity = x1.eq(&zero_el) && y1.eq(&zero_el);

    if p1_is_infinity {
        return Bytes::from([0u8; 64].to_vec());
    }
    // scalar is 0 and the point is valid
    if s.eq(&zero_u256) && BN254Curve::create_point_from_affine(x1.clone(), y1.clone()).is_ok() {
        return Bytes::from([0u8; 64].to_vec());
    }

    if let Ok(p) = BN254Curve::create_point_from_affine(x1, y1) {
        let mul = p.operate_with_self(s).to_affine();
        let res = [mul.x().to_bytes_be(), mul.y().to_bytes_be()].concat();
        return Bytes::from(res);
    }
    Bytes::new()
}

pub fn ecpairing(calldata: &Bytes, gas_limit: u64, consumed_gas: &mut u64) -> Bytes {
    let gas_cost = ECPAIRING_COST + (34_000 * (calldata.len() as u64 / 192));
    if calldata.len() % 192 != 0 || gas_limit < gas_cost {
        *consumed_gas += gas_limit;
        return Bytes::new();
    }
    *consumed_gas += gas_cost;

    let rounds = calldata.len() / 192;
    let mut mul: FieldElement<Degree12ExtensionField> = QuadraticExtensionFieldElement::one();
    for idx in 0..rounds {
        let start = idx * 192;

        // Slice lengths are checked, so unwrap is safe
        let g1_x = BN254FieldElement::from_bytes_be(&calldata[start..start + 32]).unwrap();
        let g1_y = BN254FieldElement::from_bytes_be(&calldata[start + 32..start + 64]).unwrap();

        // G2 point: ((x_0, x_1), (y_0, y_1))
        // both x and y have a real and an imaginary part of 32 bytes each
        let g2_x_bytes = [
            &calldata[start + 96..start + 128],
            &calldata[start + 64..start + 96],
        ]
        .concat();
        let g2_y_bytes = [
            &calldata[start + 160..start + 192],
            &calldata[start + 128..start + 160],
        ]
        .concat();

        let g2_x = BN254TwistCurveFieldElement::from_bytes_be(&g2_x_bytes);
        let g2_y = BN254TwistCurveFieldElement::from_bytes_be(&g2_y_bytes);

        let (g2_x, g2_y) = match (g2_x, g2_y) {
            (Ok(x), Ok(y)) => (x, y),
            _ => return Bytes::from([0u8; 32].to_vec()),
        };

        // if any point is (0,0) the pairing is ok
        let zero_el = BN254FieldElement::from(0);
        let tw_zero_el = BN254TwistCurveFieldElement::from(0);
        let p1_is_infinity = g1_x.eq(&zero_el) && g1_y.eq(&zero_el);
        let p2_is_infinity = g2_x.eq(&tw_zero_el) && g2_y.eq(&tw_zero_el);
        if p1_is_infinity || p2_is_infinity {
            continue;
        }

        let p1 = match BN254Curve::create_point_from_affine(g1_x, g1_y) {
            Ok(point) => point,
            Err(_) => return Bytes::from([0u8; 32].to_vec()),
        };
        let p2 = match BN254TwistCurve::create_point_from_affine(g2_x, g2_y) {
            Ok(point) => point,
            Err(_) => return Bytes::from([0u8; 32].to_vec()),
        };

        let pairing_result = match BN254AtePairing::compute_batch(&[(&p1, &p2)]) {
            Ok(result) => result,
            Err(_) => return Bytes::from([0u8; 32].to_vec()),
        };
        mul *= pairing_result;
    }

    let success = mul.eq(&QuadraticExtensionFieldElement::one());
    let mut output = vec![0_u8; 32];
    output[31] = success as u8;

    Bytes::from(output)
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
    use crate::primitives::U256;

    use super::*;

    #[test]
    fn modexp_gas_cost() {
        let b_size = U256::from(1_u8);
        let e_size = U256::from(1_u8);
        let m_size = U256::from(1_u8);
        let b = 8_u8;
        let e = 9_u8;
        let m = 10_u8;
        let mut calldata = [0_u8; 99];
        b_size.to_big_endian(&mut calldata[..32]);
        e_size.to_big_endian(&mut calldata[32..64]);
        m_size.to_big_endian(&mut calldata[64..96]);
        calldata[96] = b;
        calldata[97] = e;
        calldata[98] = m;

        let expected_gas = 200;
        let mut consumed_gas = 0;
        modexp(
            &Bytes::copy_from_slice(&calldata),
            expected_gas,
            &mut consumed_gas,
        );

        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn modexp_gas_cost2() {
        let b_size = U256::from(256_u16);
        let e_size = U256::from(1_u8);
        let m_size = U256::from(1_u8);
        let b = 8_u8;
        let e = 6_u8;
        let m = 10_u8;
        let mut calldata = [0_u8; 354];
        b_size.to_big_endian(&mut calldata[..32]);
        e_size.to_big_endian(&mut calldata[32..64]);
        m_size.to_big_endian(&mut calldata[64..96]);
        calldata[351] = b;
        calldata[352] = e;
        calldata[353] = m;

        let expected_gas = 682;
        let mut consumed_gas = 0;
        modexp(
            &Bytes::copy_from_slice(&calldata),
            expected_gas,
            &mut consumed_gas,
        );

        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecadd_happy_path() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
        );
        let expected_gas = ECADD_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let expected_x =
            hex::decode("030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd3")
                .unwrap();
        let expected_y =
            hex::decode("15ed738c0e0a7c92e7845f96b2ae9c0a68a6a449e3538fc7ff3ebf7a5a18a2c4")
                .unwrap();
        let expected_result = Bytes::from([expected_x, expected_y].concat());
        let result = ecadd(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecadd_infinity_with_valid_point() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
        );
        let expected_gas = ECADD_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let expected_x =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();
        let expected_y =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000002")
                .unwrap();
        let expected_result = Bytes::from([expected_x, expected_y].concat());
        let result = ecadd(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecadd_valid_point_with_infinity() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
        );
        let expected_gas = ECADD_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let expected_x =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();
        let expected_y =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000002")
                .unwrap();
        let expected_result = Bytes::from([expected_x, expected_y].concat());
        let result = ecadd(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecadd_infinity_twice() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
        );
        let expected_gas = ECADD_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let result = ecadd(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, Bytes::from([0u8; 64].to_vec()));
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecadd_with_invalid_first_point() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
        );
        let expected_gas = ECADD_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let result = ecadd(&calldata, gas_limit, &mut consumed_gas);

        assert!(result.is_empty());
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecadd_with_invalid_second_point() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000001",
            )
            .unwrap(),
        );
        let expected_gas = ECADD_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let result = ecadd(&calldata, gas_limit, &mut consumed_gas);

        assert!(result.is_empty());
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecadd_with_invalid_calldata() {
        // calldata's len = 127
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002\
            0000000000000000000000000000000000000000000000000000000000000001\
            00000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
        );
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let result = ecadd(&calldata, gas_limit, &mut consumed_gas);

        assert!(result.is_empty());
        assert_eq!(consumed_gas, gas_limit);
    }

    #[test]
    fn ecadd_with_not_enough_gas() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
        );
        let gas_limit = 149;
        let mut consumed_gas = 0;

        let result = ecadd(&calldata, gas_limit, &mut consumed_gas);

        assert!(result.is_empty());
        assert_eq!(consumed_gas, gas_limit);
    }

    #[test]
    fn ecmul_happy_path() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002\
            0000000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
        );
        let expected_gas = ECMUL_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let expected_x =
            hex::decode("030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd3")
                .unwrap();
        let expected_y =
            hex::decode("15ed738c0e0a7c92e7845f96b2ae9c0a68a6a449e3538fc7ff3ebf7a5a18a2c4")
                .unwrap();
        let expected_result = Bytes::from([expected_x, expected_y].concat());
        let result = ecmul(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecmul_infinity() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
        );
        let expected_gas = ECMUL_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let result = ecmul(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, Bytes::from([0u8; 64].to_vec()));
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecmul_by_zero() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002\
            0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
        );
        let expected_gas = ECMUL_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let result = ecmul(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, Bytes::from([0u8; 64].to_vec()));
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecmul_invalid_point() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
        );
        let expected_gas = ECMUL_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let result = ecmul(&calldata, gas_limit, &mut consumed_gas);

        assert!(result.is_empty());
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecmul_invalid_point_by_zero() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
        );
        let expected_gas = ECMUL_COST;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let result = ecmul(&calldata, gas_limit, &mut consumed_gas);

        assert!(result.is_empty());
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecmul_with_invalid_calldata() {
        // calldata's len = 95
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002\
            00000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
        );
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let result = ecmul(&calldata, gas_limit, &mut consumed_gas);

        assert!(result.is_empty());
        assert_eq!(consumed_gas, gas_limit);
    }

    #[test]
    fn ecmul_with_not_enough_gas() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000001\
            0000000000000000000000000000000000000000000000000000000000000002\
            0000000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
        );
        let gas_limit = 149;
        let mut consumed_gas = 0;

        let result = ecmul(&calldata, gas_limit, &mut consumed_gas);

        assert!(result.is_empty());
        assert_eq!(consumed_gas, gas_limit);
    }

    #[test]
    fn ecpairing_happy_path() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            2cf44499d5d27bb186308b7af7af02ac5bc9eeb6a3d147c186b21fb1b76e18da\
            2c0f001f52110ccfe69108924926e45f0b0c868df0e7bde1fe16d3242dc715f6\
            1fb19bb476f6b9e44e2a32234da8212f61cd63919354bc06aef31e3cfaff3ebc\
            22606845ff186793914e03e21df544c34ffe2f2f3504de8a79d9159eca2d98d9\
            2bd368e28381e8eccb5fa81fc26cf3f048eea9abfdd85d7ed3ab3698d63e4f90\
            2fe02e47887507adf0ff1743cbac6ba291e66f59be6bd763950bb16041a0a85e\
            0000000000000000000000000000000000000000000000000000000000000001\
            30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd45\
            1971ff0471b09fa93caaf13cbf443c1aede09cc4328f5a62aad45f40ec133eb4\
            091058a3141822985733cbdddfed0fd8d6c104e9e9eff40bf5abfef9ab163bc7\
            2a23af9a5ce2ba2796c1f4e453a370eb0af8c212d9dc9acd8fc02c2e907baea2\
            23a8eb0b0996252cb548a4487da97b02422ebc0e834613f954de6c7e0afdc1fc",
            )
            .unwrap(),
        );
        let expected_gas = 113_000;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let expected_result = Bytes::from(
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap(),
        );
        let result = ecpairing(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecpairing_invalid_points() {
        // changed last byte from `fc` to `fd`
        let calldata = Bytes::from(
            hex::decode(
                "\
            2cf44499d5d27bb186308b7af7af02ac5bc9eeb6a3d147c186b21fb1b76e18da\
            2c0f001f52110ccfe69108924926e45f0b0c868df0e7bde1fe16d3242dc715f6\
            1fb19bb476f6b9e44e2a32234da8212f61cd63919354bc06aef31e3cfaff3ebc\
            22606845ff186793914e03e21df544c34ffe2f2f3504de8a79d9159eca2d98d9\
            2bd368e28381e8eccb5fa81fc26cf3f048eea9abfdd85d7ed3ab3698d63e4f90\
            2fe02e47887507adf0ff1743cbac6ba291e66f59be6bd763950bb16041a0a85e\
            0000000000000000000000000000000000000000000000000000000000000001\
            30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd45\
            1971ff0471b09fa93caaf13cbf443c1aede09cc4328f5a62aad45f40ec133eb4\
            091058a3141822985733cbdddfed0fd8d6c104e9e9eff40bf5abfef9ab163bc7\
            2a23af9a5ce2ba2796c1f4e453a370eb0af8c212d9dc9acd8fc02c2e907baea2\
            23a8eb0b0996252cb548a4487da97b02422ebc0e834613f954de6c7e0afdc1fd",
            )
            .unwrap(),
        );
        let expected_gas = 113_000;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let expected_result = Bytes::from([0u8; 32].to_vec());
        let result = ecpairing(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecpairing_p1_is_infinity() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            1fb19bb476f6b9e44e2a32234da8212f61cd63919354bc06aef31e3cfaff3ebc\
            22606845ff186793914e03e21df544c34ffe2f2f3504de8a79d9159eca2d98d9\
            2bd368e28381e8eccb5fa81fc26cf3f048eea9abfdd85d7ed3ab3698d63e4f90\
            2fe02e47887507adf0ff1743cbac6ba291e66f59be6bd763950bb16041a0a85e",
            )
            .unwrap(),
        );
        let expected_gas = 79_000;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let expected_result = Bytes::from(
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap(),
        );
        let result = ecpairing(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecpairing_p2_is_infinity() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            2cf44499d5d27bb186308b7af7af02ac5bc9eeb6a3d147c186b21fb1b76e18da\
            2c0f001f52110ccfe69108924926e45f0b0c868df0e7bde1fe16d3242dc715f6\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
        );
        let expected_gas = 79_000;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let expected_result = Bytes::from(
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap(),
        );
        let result = ecpairing(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_gas);
    }
    #[test]
    fn ecpairing_out_of_curve() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            1111111111111111111111111111111111111111111111111111111111111111\
            1111111111111111111111111111111111111111111111111111111111111111\
            1111111111111111111111111111111111111111111111111111111111111111\
            1111111111111111111111111111111111111111111111111111111111111111\
            1111111111111111111111111111111111111111111111111111111111111111\
            1111111111111111111111111111111111111111111111111111111111111111",
            )
            .unwrap(),
        );
        let expected_gas = 79_000;
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;

        let expected_result = Bytes::from([0u8; 32].to_vec());
        let result = ecpairing(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_gas);
    }

    #[test]
    fn ecpairing_invalid_calldata() {
        let calldata = Bytes::from(
            hex::decode(
                "\
                1111111111111111111111111111111111111111111111111111111111111111",
            )
            .unwrap(),
        );
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;
        let expected_result = Bytes::new();
        let result = ecpairing(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, gas_limit);
    }

    #[test]
    fn ecpairing_empty_calldata() {
        let calldata = Bytes::new();
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;
        let expected_result = Bytes::from(
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap(),
        );
        let result = ecpairing(&calldata, gas_limit, &mut consumed_gas);

        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, ECPAIRING_COST);
    }

    #[test]
    fn ecpairing_with_not_enough_gas() {
        let calldata = Bytes::from(
            hex::decode(
                "\
            2cf44499d5d27bb186308b7af7af02ac5bc9eeb6a3d147c186b21fb1b76e18da\
            2c0f001f52110ccfe69108924926e45f0b0c868df0e7bde1fe16d3242dc715f6\
            1fb19bb476f6b9e44e2a32234da8212f61cd63919354bc06aef31e3cfaff3ebc\
            22606845ff186793914e03e21df544c34ffe2f2f3504de8a79d9159eca2d98d9\
            2bd368e28381e8eccb5fa81fc26cf3f048eea9abfdd85d7ed3ab3698d63e4f90\
            2fe02e47887507adf0ff1743cbac6ba291e66f59be6bd763950bb16041a0a85e\
            0000000000000000000000000000000000000000000000000000000000000001\
            30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd45\
            1971ff0471b09fa93caaf13cbf443c1aede09cc4328f5a62aad45f40ec133eb4\
            091058a3141822985733cbdddfed0fd8d6c104e9e9eff40bf5abfef9ab163bc7\
            2a23af9a5ce2ba2796c1f4e453a370eb0af8c212d9dc9acd8fc02c2e907baea2\
            23a8eb0b0996252cb548a4487da97b02422ebc0e834613f954de6c7e0afdc1fc",
            )
            .unwrap(),
        );
        // needs 113_000
        let gas_limit = 100_000;
        let mut consumed_gas = 0;

        let result = ecpairing(&calldata, gas_limit, &mut consumed_gas);

        assert!(result.is_empty());
        assert_eq!(consumed_gas, gas_limit);
    }

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
