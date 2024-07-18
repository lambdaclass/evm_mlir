use crate::primitives::U256;
use bytes::Bytes;
use num_bigint::BigUint;
use secp256k1::{ecdsa, Message, Secp256k1};
use sha3::{Digest, Keccak256};

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
    let gas_cost = 0; // TODO: add gas cost
    if gas_limit < gas_cost {
        return Bytes::new();
    }
    *consumed_gas += gas_cost;

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

    // TODO: casting exp as u32, change pow to a more powerful method
    // Maybe https://docs.rs/aurora-engine-modexp/latest/aurora_engine_modexp/
    let e: u32 = e.try_into().unwrap();

    let result = b.pow(e) % m;

    let output = &result.to_bytes_be()[..m_size]; // test if [..m_size] indexes correctly for bigger return values
    Bytes::copy_from_slice(output)
}
