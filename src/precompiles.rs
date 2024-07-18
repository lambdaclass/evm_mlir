use bytes::Bytes;
use secp256k1::{ecdsa, Message, Secp256k1};
use sha3::{Digest, Keccak256};

use crate::constants::precompiles::ECRECOVER_COST;

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