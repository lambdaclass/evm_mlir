use crate::constants::precompiles::{
    blake2_gas_cost, identity_dynamic_cost, ripemd_160_dynamic_cost, sha2_256_dynamic_cost,
    ECRECOVER_COST, IDENTITY_COST, POINT_EVAL_COST, RIPEMD_160_COST, SHA2_256_COST,
};
use crate::primitives::U256;
use bytes::Bytes;
use hex;
use lambdaworks_crypto::commitments::kzg::{KateZaveruchaGoldberg, StructuredReferenceString};
use lambdaworks_crypto::commitments::traits::IsCommitmentScheme;
use lambdaworks_math::cyclic_group::IsGroup;
use lambdaworks_math::elliptic_curve::short_weierstrass::curves::bls12_381::compression::BLS12381FieldElement;
use lambdaworks_math::elliptic_curve::short_weierstrass::curves::bls12_381::field_extension::Degree2ExtensionField;
use lambdaworks_math::elliptic_curve::short_weierstrass::curves::bls12_381::pairing::BLS12381AtePairing;
use lambdaworks_math::elliptic_curve::short_weierstrass::curves::bls12_381::sqrt;
use lambdaworks_math::elliptic_curve::traits::FromAffine;
use lambdaworks_math::elliptic_curve::{
    short_weierstrass::{
        curves::bls12_381::{
            curve::BLS12381Curve,
            default_types::{FrElement, FrField},
            twist::BLS12381TwistCurve,
        },
        point::ShortWeierstrassProjectivePoint,
    },
    traits::IsEllipticCurve,
};
use lambdaworks_math::field::element::FieldElement;
use lambdaworks_math::traits::ByteConversion;
use num_bigint::BigUint;
use secp256k1::{ecdsa, Message, Secp256k1};
use sha3::{Digest, Keccak256};
use std::array::TryFromSliceError;
use std::io;

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

type G1 = ShortWeierstrassProjectivePoint<BLS12381Curve>;
type G1Point = ShortWeierstrassProjectivePoint<BLS12381Curve>;
type G2Point = <BLS12381TwistCurve as IsEllipticCurve>::PointRepresentation;
type KZG = KateZaveruchaGoldberg<FrField, BLS12381AtePairing>;

const BYTES_PER_G1_POINT: usize = 48;
const BYTES_PER_G2_POINT: usize = 96;
const POINT_EVAL_CALLDATA_LEN: usize = 192;

enum EllipticCurveError {
    InvalidPoint,
}

fn load_trusted_setup_to_points() -> io::Result<(Vec<G1>, Vec<G2Point>)> {
    // https://github.com/lambdaclass/lambdaworks_kzg/blob/8f031b1f32e170c1af06029e46c73404f4c85e2e/src/srs.rs#L25
    let lines = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/official_trusted_setup.txt"
    ));
    let mut lines = lines.lines();

    let mut g1_bytes: [u8; BYTES_PER_G1_POINT] = [0; BYTES_PER_G1_POINT];
    let mut g1_points: Vec<G1> = Vec::new();

    let mut g2_bytes: [u8; BYTES_PER_G2_POINT] = [0; BYTES_PER_G2_POINT];
    let mut g2_points: Vec<G2Point> = Vec::new();

    // Read the number of g1 points
    let num_g1_points = lines
        .next()
        .ok_or(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid file format",
        ))?
        .parse::<usize>()
        .map_err(|_| std::io::ErrorKind::InvalidData)?;

    let num_g2_points = lines
        .next()
        .ok_or(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid file format",
        ))?
        .parse::<usize>()
        .map_err(|_| std::io::ErrorKind::InvalidData)?;

    let num_total_points = num_g1_points + num_g2_points;

    // read all g1 points
    for (pos, line) in lines.enumerate() {
        if pos < num_g1_points {
            // read g1 point
            hex::decode_to_slice(line, &mut g1_bytes)
                .map_err(|_| std::io::ErrorKind::InvalidData)?;

            let g1_point =
                decompress_g1_point(&mut g1_bytes).map_err(|_| std::io::ErrorKind::InvalidData)?;

            g1_points.push(g1_point);
        } else if pos < num_total_points {
            // read g2 point
            hex::decode_to_slice(line, &mut g2_bytes)
                .map_err(|_| std::io::ErrorKind::InvalidData)?;

            let g2_point =
                decompress_g2_point(&mut g2_bytes).map_err(|_| std::io::ErrorKind::InvalidData)?;

            g2_points.push(g2_point);
        } else {
            // all the points were already parsed
            break;
        }
    }

    Ok((g1_points, g2_points))
}

fn points_to_structured_reference_string(
    g1_points: &[G1],
    g2_points: &[G2Point],
) -> StructuredReferenceString<G1, G2Point> {
    let g2_points_arr = [g2_points[0].clone(), g2_points[1].clone()];

    StructuredReferenceString::new(g1_points, &g2_points_arr)
}

fn decompress_g1_point(input_bytes: &mut [u8; 48]) -> Result<G1Point, EllipticCurveError> {
    let first_byte = input_bytes.first().unwrap();
    // We get the first 3 bits
    let prefix_bits = first_byte >> 5;
    let first_bit = (prefix_bits & 4_u8) >> 2;
    // If first bit is not 1, then the value is not compressed.
    if first_bit != 1 {
        return Err(EllipticCurveError::InvalidPoint);
    }
    let second_bit = (prefix_bits & 2_u8) >> 1;
    // If the second bit is 1, then the compressed point is the
    // point at infinity and we return it directly.
    if second_bit == 1 {
        return Ok(G1Point::neutral_element());
    }
    let third_bit = prefix_bits & 1_u8;

    let first_byte_without_contorl_bits = (first_byte << 3) >> 3;
    input_bytes[0] = first_byte_without_contorl_bits;

    let x = BLS12381FieldElement::from_bytes_be(input_bytes)
        .map_err(|_e| EllipticCurveError::InvalidPoint)?;

    // We apply the elliptic curve formula to know the y^2 value.
    let y_squared = x.pow(3_u16) + BLS12381FieldElement::from(4);

    let (y_sqrt_1, y_sqrt_2) = &y_squared.sqrt().ok_or(EllipticCurveError::InvalidPoint)?;

    // we call "negative" to the greate root,
    // if the third bit is 1, we take this grater value.
    // Otherwise, we take the second one.
    let y = sqrt::select_sqrt_value_from_third_bit(y_sqrt_1.clone(), y_sqrt_2.clone(), third_bit);
    let point = G1Point::from_affine(x, y).map_err(|_| EllipticCurveError::InvalidPoint)?;

    point
        .is_in_subgroup()
        .then_some(point)
        .ok_or(EllipticCurveError::InvalidPoint)
}

fn decompress_g2_point(input_bytes: &mut [u8; 96]) -> Result<G2Point, EllipticCurveError> {
    let first_byte = input_bytes.first().unwrap();

    // We get the first 3 bits
    let prefix_bits = first_byte >> 5;
    let first_bit = (prefix_bits & 4_u8) >> 2;
    // If first bit is not 1, then the value is not compressed.
    if first_bit != 1 {
        return Err(EllipticCurveError::InvalidPoint);
    }
    let second_bit = (prefix_bits & 2_u8) >> 1;
    // If the second bit is 1, then the compressed point is the
    // point at infinity and we return it directly.
    if second_bit == 1 {
        return Ok(G2Point::neutral_element());
    }

    let first_byte_without_contorl_bits = (first_byte << 3) >> 3;
    input_bytes[0] = first_byte_without_contorl_bits;

    let input0 = &input_bytes[48..];
    let input1 = &input_bytes[0..48];
    let x0 = BLS12381FieldElement::from_bytes_be(input0).unwrap();
    let x1 = BLS12381FieldElement::from_bytes_be(input1).unwrap();
    let x: FieldElement<Degree2ExtensionField> = FieldElement::new([x0, x1]);

    const VALUE: BLS12381FieldElement = BLS12381FieldElement::from_hex_unchecked("4");
    let b_param_qfe = FieldElement::<Degree2ExtensionField>::new([VALUE, VALUE]);

    let y =
        sqrt::sqrt_qfe(&(x.pow(3_u64) + b_param_qfe), 0).ok_or(EllipticCurveError::InvalidPoint)?;

    G2Point::from_affine(x, y).map_err(|_| EllipticCurveError::InvalidPoint)
}

pub const VERSIONED_HASH_VERSION_KZG: u8 = 0x01;

pub fn kzg_to_versioned_hash(commitment: &[u8]) -> [u8; 32] {
    let mut hash: [u8; 32] = sha2::Sha256::digest(commitment).into();
    hash[0] = VERSIONED_HASH_VERSION_KZG;
    hash
}

// Return FIELD_ELEMENTS_PER_BLOB and BLS_MODULUS as padded 32 byte big endian values.
// FIELD_ELEMENTS_PER_BLOB = 4096 = 0x1000;
// BLS_MODULUS = 52435875175126190479447740508185965837690552500527637822603658699938581184513
// = 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001;
const POINT_EVAL_RETURN: &str =
    "000000000000000000000000000000000000000000000000000000000000100073eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001";

// TODO: alias as the actual return type (bytes encoded).
// TODO: define better error.
#[derive(Debug, PartialEq)]
pub enum PointEvalErr {
    NotEnoughGas,
    CalldataLengthInvalid,
    CalldataParseError,
    MismatchedVersionedHash,
    PointDecompressionError,
    TrustedSetupError,
    VerificationFalse,
}

pub fn point_eval(
    calldata: &Bytes,
    gas_limit: u64,
    consumed_gas: &mut u64,
) -> Result<Bytes, PointEvalErr> {
    if gas_limit < POINT_EVAL_COST {
        return Err(PointEvalErr::NotEnoughGas);
    }

    if calldata.len() != POINT_EVAL_CALLDATA_LEN {
        return Err(PointEvalErr::CalldataLengthInvalid);
    }

    *consumed_gas += POINT_EVAL_COST;

    /*
       The calldata is encoded as follows:

       RANGE        NAME            DESCRIPTION
       [0: 32]      versioned_hash  Reference to a blob in the execution layer.
       [32: 64]     x               x-coordinate at which the blob is being evaluated.
       [64: 96]     y               y-coordinate at which the blob is being evaluated.
       [96: 144]    commitment      Commitment to the blob being evaluated
       [144: 192]   proof           Proof associated with the commitment
    */
    let versioned_hash: &[u8; 32] = &calldata[..32]
        .try_into()
        .map_err(|_| PointEvalErr::CalldataParseError)?;
    let mut commitment: [u8; 48] = calldata[96..144]
        .try_into()
        .map_err(|_| PointEvalErr::CalldataParseError)?;

    if kzg_to_versioned_hash(&commitment) != *versioned_hash {
        return Err(PointEvalErr::MismatchedVersionedHash);
    }

    let x: &[u8; 32] = &calldata[32..64]
        .try_into()
        .map_err(|_| PointEvalErr::CalldataParseError)?;
    let y: &[u8; 32] = &calldata[64..96]
        .try_into()
        .map_err(|_| PointEvalErr::CalldataParseError)?;
    let mut proof: [u8; 48] = calldata[144..192]
        .try_into()
        .map_err(|_| PointEvalErr::CalldataParseError)?;

    let x_fr = FrElement::from_bytes_be(x).map_err(|_| PointEvalErr::CalldataParseError)?;
    let y_fr = FrElement::from_bytes_be(y).map_err(|_| PointEvalErr::CalldataParseError)?;

    let commitment_g1 =
        decompress_g1_point(&mut commitment).map_err(|_| PointEvalErr::PointDecompressionError)?;
    let proof_g1 =
        decompress_g1_point(&mut proof).map_err(|_| PointEvalErr::PointDecompressionError)?;

    let (g1_points, g2_points) =
        load_trusted_setup_to_points().map_err(|_| PointEvalErr::TrustedSetupError)?;
    let srs = points_to_structured_reference_string(&g1_points, &g2_points);
    let kzg = KZG::new(srs);

    match kzg.verify(&x_fr, &y_fr, &commitment_g1, &proof_g1) {
        false => Err(PointEvalErr::VerificationFalse),
        true => Ok(Bytes::copy_from_slice(POINT_EVAL_RETURN.as_bytes())),
    }
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
