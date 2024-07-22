use crate::{
    constants::precompiles::{ECADD_COST, ECMUL_COST, ECPAIRING_COST},
    primitives::U256,
};
use bytes::Bytes;
use num_bigint::BigUint;
use secp256k1::{ecdsa, Message, Secp256k1};
use sha3::{Digest, Keccak256};
use substrate_bn::{pairing, AffineG1, AffineG2, Fq, Fq2, Fr, Group, Gt, G1, G2};

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
    if calldata.len() < 96 || gas_limit < ECMUL_COST {
        *consumed_gas += gas_limit;
        return Bytes::new();
    }
    *consumed_gas += ECMUL_COST;

    // Slice lengths are checked, so unwrap is safe
    let x1 = Fq::from_slice(&calldata[..32]).unwrap();
    let y1 = Fq::from_slice(&calldata[32..64]).unwrap();
    let s = Fr::from_slice(&calldata[64..96]).unwrap();
    // TODO: check for infinity results

    let p: G1 = AffineG1::new(x1, y1).unwrap().into(); // handle unwrap

    let Some(sum) = AffineG1::from_jacobian(p * s) else {
        *consumed_gas += gas_limit - ECMUL_COST;
        return Bytes::new();
    };
    let mut output = [0_u8; 64];
    sum.x().to_big_endian(&mut output[..32]).unwrap();
    sum.y().to_big_endian(&mut output[32..]).unwrap();

    return Bytes::copy_from_slice(&output);
}

pub fn ecpairing(calldata: &Bytes, gas_limit: u64, consumed_gas: &mut u64) -> Bytes {
    let gas_cost = ECPAIRING_COST + (34_000 * (calldata.len() as u64 / 192));
    if calldata.len() % 192 != 0 || gas_limit < gas_cost {
        *consumed_gas += gas_limit;
        return Bytes::new();
    }
    *consumed_gas += gas_cost;

    let rounds = calldata.len() / 192;
    let mut mul = Gt::one();
    for idx in 0..rounds {
        let start = idx * 192;

        let x1 = Fq::from_slice(&calldata[start..start + 32]).unwrap();
        let y1 = Fq::from_slice(&calldata[start + 32..start + 64]).unwrap();
        let x2 = Fq::from_slice(&calldata[start + 64..start + 96]).unwrap();
        let y2 = Fq::from_slice(&calldata[start + 96..start + 128]).unwrap();
        let x3 = Fq::from_slice(&calldata[start + 128..start + 160]).unwrap();
        let y3 = Fq::from_slice(&calldata[start + 160..start + 192]).unwrap();

        let p1: G1 = if x1.is_zero() && y1.is_zero() {
            G1::zero()
        } else {
            G1::from(AffineG1::new(x1, y1).unwrap())
        };
        let p2 = Fq2::new(y2, x2); // TODO: check if this is OK, only works using (y,x) instead of (x,y)
        let p3 = Fq2::new(y3, x3);

        let b = if p2.is_zero() && p3.is_zero() {
            G2::zero()
        } else {
            G2::from(AffineG2::new(p2, p3).unwrap())
        };
        mul = mul * pairing(p1, b);
    }

    let success = mul == Gt::one();
    let mut output = [0_u8; 32];
    output[31] = success as u8;
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

    #[test]
    fn ecpairing_happy_path() {
        let params = "\
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
            23a8eb0b0996252cb548a4487da97b02422ebc0e834613f954de6c7e0afdc1fc";
        let expected_gas = 113000;

        let calldata = hex::decode(params).unwrap();
        let gas_limit = 100_000_000;
        let mut consumed_gas = 0;
        let mut res = [0_u8; 32];
        res[31] = 1;
        let expected_result = Bytes::copy_from_slice(&res);

        let result = ecpairing(
            &Bytes::copy_from_slice(&calldata),
            gas_limit,
            &mut consumed_gas,
        );
        assert_eq!(result, expected_result);
        assert_eq!(consumed_gas, expected_gas);
    }

    // TODO: add tests for not in curve cases, and zero values
}
