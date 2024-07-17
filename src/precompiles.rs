use bytes::Bytes;
use secp256k1::{ecdsa, Message, Secp256k1};
use sha3::{Digest, Keccak256};

pub fn ecrecover(calldata: &Bytes) -> Result<Bytes, secp256k1::Error> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ecrecover() {
        let hash = hex::decode("456e9aea5e197a1f1af7a3e85a3212fa4049a3ba34c2289b4c860fc0b0c64ef3")
            .unwrap();
        let mut v = [0; 32];
        v[31] = 28;
        let r = hex::decode("9242685bf161793cc25603c231bc2f568eb630ea16aa137d2664ac8038825608")
            .unwrap();
        let s = hex::decode("4f8ae3bd7535248d0bd448298cc2e2071e56992d0774dc340c368ae950852ada")
            .unwrap();

        let mut calldata = Vec::<u8>::new();
        calldata.extend(hash);
        calldata.extend(v);
        calldata.extend(r);
        calldata.extend(s);

        let result = ecrecover(&Bytes::from(calldata)).unwrap();
        let expected_result = Bytes::from(
            hex::decode("0000000000000000000000007156526fbd7a3c72969b54f64e42c10fbb768c8a")
                .unwrap(),
        );

        assert_eq!(result, expected_result);
    }
}
