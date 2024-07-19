use crate::{
    constants::precompiles::{ECADD_COST, ECMUL_COST},
    primitives::U256,
};
use bytes::Bytes;
use num_bigint::BigUint;
use secp256k1::{ecdsa, Message, Secp256k1};
use sha3::{Digest, Keccak256};
use substrate_bn::{AffineG1, Fq, Fr, G1};

use crate::constants::precompiles::{
    identity_dynamic_cost, ripemd_160_dynamic_cost, sha2_256_dynamic_cost, ECRECOVER_COST,
    IDENTITY_COST, RIPEMD_160_COST, SHA2_256_COST,
};

pub fn ecrecover(
    calldata: &Bytes,
    gas_limit: u64,
    consumed_gas: &mut u64,
) -> Result<Bytes, secp256k1::Error> {
    if gas_limit < ECRECOVER_COST || calldata.len() < 128 {
        return Ok(Bytes::from_static(&[0_u8; 32]));
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
        // TODO: casting exp as u32, change pow to a more powerful method
        // Maybe https://docs.rs/aurora-engine-modexp/latest/aurora_engine_modexp/
        let e: u32 = e.try_into().unwrap();
        b.pow(e) % m
    };

    let output = &result.to_bytes_be()[..m_size];
    Bytes::copy_from_slice(output)
}

pub fn ecadd(calldata: &Bytes, gas_limit: u64, consumed_gas: &mut u64) -> Bytes {
    if calldata.len() < 128 {
        *consumed_gas += gas_limit;
        return Bytes::new();
    }
    *consumed_gas += ECADD_COST;

    // Slice lengths are checked, so unwrap is safe
    let x1 = Fq::from_slice(&calldata[..32]).unwrap();
    let y1 = Fq::from_slice(&calldata[32..64]).unwrap();
    let x2 = Fq::from_slice(&calldata[64..96]).unwrap();
    let y2 = Fq::from_slice(&calldata[96..128]).unwrap();

    let p1: G1 = AffineG1::new(x1, y1).unwrap().into();
    let p2: G1 = AffineG1::new(x2, y2).unwrap().into();

    let Some(sum) = AffineG1::from_jacobian(p1 + p2) else {
        *consumed_gas += gas_limit - ECADD_COST;
        return Bytes::new();
    };
    let mut output = [0_u8; 64];
    sum.x().to_big_endian(&mut output[..32]).unwrap();
    sum.y().to_big_endian(&mut output[32..]).unwrap();

    return Bytes::copy_from_slice(&output);
}

pub fn ecmul(calldata: &Bytes, gas_limit: u64, consumed_gas: &mut u64) -> Bytes {
    if calldata.len() < 96 {
        *consumed_gas += gas_limit;
        return Bytes::new();
    }
    *consumed_gas += ECMUL_COST;

    // Slice lengths are checked, so unwrap is safe
    let x1 = Fq::from_slice(&calldata[..32]).unwrap();
    let y1 = Fq::from_slice(&calldata[32..64]).unwrap();
    let s = Fr::from_slice(&calldata[64..96]).unwrap();

    let p: G1 = AffineG1::new(x1, y1).unwrap().into();

    let Some(sum) = AffineG1::from_jacobian(p * s) else {
        *consumed_gas += gas_limit - ECMUL_COST;
        return Bytes::new();
    };
    let mut output = [0_u8; 64];
    sum.x().to_big_endian(&mut output[..32]).unwrap();
    sum.y().to_big_endian(&mut output[32..]).unwrap();

    return Bytes::copy_from_slice(&output);
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
}
