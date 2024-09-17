use std::{collections::HashMap, path::Path};

use bytes::Bytes;
use evm_mlir::{db::Db, env::TransactTo, result::ExecutionResult, Env, Evm};

use super::models::{AccountInfo, Test, TestSuite, TestUnit};

/// Receives a Bytes object with the hex representation
/// And returns a Bytes object with the decimal representation
/// Taking the hex numbers by pairs
fn decode_hex(bytes_in_hex: Bytes) -> Option<Bytes> {
    let hex_header = &bytes_in_hex[0..2];
    if hex_header != b"0x" {
        return None;
    }
    let hex_string = std::str::from_utf8(&bytes_in_hex[2..]).unwrap(); // we don't need the 0x
    let mut opcodes = Vec::new();
    for i in (0..hex_string.len()).step_by(2) {
        let pair = &hex_string[i..i + 2];
        let value = u8::from_str_radix(pair, 16).unwrap();
        opcodes.push(value);
    }
    Some(Bytes::from(opcodes))
}

fn setup_evm(test: &Test, unit: &TestUnit) -> Evm<Db> {
    let to = match unit.transaction.to {
        Some(to) => TransactTo::Call(to),
        None => TransactTo::Create,
    };
    let sender = unit.transaction.sender.unwrap_or_default();
    let gas_price = unit.transaction.gas_price.unwrap_or_default();
    let mut env = Env::default();
    env.tx.transact_to = to.clone();
    env.tx.gas_price = unit
        .transaction
        .gas_price
        .or(unit.transaction.max_fee_per_gas)
        .unwrap_or_default();
    env.tx.caller = sender;
    env.tx.gas_limit = unit.transaction.gas_limit[test.indexes.gas].as_u64();
    env.tx.value = unit.transaction.value[test.indexes.value];
    env.tx.data = decode_hex(unit.transaction.data[test.indexes.data].clone()).unwrap();
    env.tx
        .blob_hashes
        .clone_from(&unit.transaction.blob_versioned_hashes);
    env.tx.max_fee_per_blob_gas = unit.transaction.max_fee_per_blob_gas;

    env.block.number = unit.env.current_number;
    env.block.coinbase = unit.env.current_coinbase;
    env.block.timestamp = unit.env.current_timestamp;
    let excess_blob_gas = unit
        .env
        .current_excess_blob_gas
        .unwrap_or_default()
        .as_u64();
    env.block.set_blob_base_fee(excess_blob_gas);

    if let Some(basefee) = unit.env.current_base_fee {
        env.block.basefee = basefee;
    };
    // let mut db = Db::new();
    let mut db = match to.clone() {
        TransactTo::Call(to) => {
            let opcodes = decode_hex(
                unit.pre
                    .get(&to)
                    .unwrap_or(&AccountInfo::default())
                    .code
                    .clone(),
            )
            .unwrap();
            Db::new().with_contract(to, opcodes)
        }
        TransactTo::Create => {
            let opcodes = decode_hex(unit.pre.get(&env.tx.caller).unwrap().code.clone()).unwrap();
            Db::new().with_contract(env.tx.get_address(), opcodes)
        }
    };

    // Load pre storage into db
    for (address, account_info) in unit.pre.iter() {
        let opcodes = decode_hex(account_info.code.clone()).unwrap();
        db = db.with_contract(address.to_owned(), opcodes);
        db.set_account(
            address.to_owned(),
            account_info.nonce,
            account_info.balance,
            account_info.storage.clone(),
        );
    }

    Evm::new(env, db)
}

fn verify_result(
    test: &Test,
    expected_result: Option<&Bytes>,
    execution_result: &ExecutionResult,
) -> Result<(), String> {
    match (&test.expect_exception, execution_result) {
        (None, _) => {
            // We need to do the .zip as some tests of the ef returns "None" as expected when the results are big
            if let Some((expected_output, output)) = expected_result.zip(execution_result.output())
            {
                if expected_output != output {
                    return Err("Wrong output".into());
                }
            }
            Ok(())
        }
        (Some(_), ExecutionResult::Halt { .. } | ExecutionResult::Revert { .. }) => {
            Ok(()) //Halt/Revert and want an error
        }
        _ => Err("Expected exception but got none".into()),
    }
}

/// Test the resulting storage is the same as the expected storage
fn verify_storage(
    post_state: &HashMap<ethereum_types::H160, AccountInfo>,
    res_state: HashMap<ethereum_types::H160, evm_mlir::state::Account>,
) {
    let mut result_state = HashMap::new();
    for address in post_state.keys() {
        let account = res_state.get(address).unwrap();
        let opcodes = decode_hex(account.info.code.clone().unwrap()).unwrap();
        result_state.insert(
            address.to_owned(),
            AccountInfo {
                balance: account.info.balance,
                code: opcodes,
                nonce: account.info.nonce,
                storage: account
                    .storage
                    .clone()
                    .into_iter()
                    .map(|(addr, slot)| (addr, slot.present_value))
                    .collect(),
            },
        );
    }
    assert_eq!(*post_state, result_state);
}

pub fn run_test(path: &Path, contents: String) -> datatest_stable::Result<()> {
    let test_suite: TestSuite = serde_json::from_reader(contents.as_bytes())
        .unwrap_or_else(|_| panic!("Failed to parse JSON test {}", path.display()));

    for (_name, unit) in test_suite.0 {
        // NOTE: currently we only support Cancun spec
        let Some(tests) = unit.post.get("Cancun") else {
            continue;
        };

        for test in tests {
            let mut evm = setup_evm(test, &unit);
            let res = evm.transact();

            // Tests if the error was expected by the test.
            // TODO: if many more tests return these "named exceptions",
            // like "TR_BLOBLIST_OVERSIZE", it could be useful to have a
            // utils function to map them to some error types used here.
            match (&test.expect_exception.as_deref(), &res) {
                (
                    Some("TR_BLOBLIST_OVERSIZE"),
                    Err(EVMError::Transaction(InvalidTransaction::TooManyBlobs { .. })),
                ) => return Ok(()),
                (
                    Some("TR_BLOBCREATE"),
                    Err(EVMError::Transaction(InvalidTransaction::BlobCreateTransaction)),
                ) => return Ok(()),
                (
                    Some("TR_BLOBVERSION_INVALID"),
                    Err(EVMError::Transaction(InvalidTransaction::BlobVersionNotSupported)),
                ) => return Ok(()),
                (
                    Some("TransactionException.TYPE_3_TX_INVALID_BLOB_VERSIONED_HASH"),
                    Err(EVMError::Transaction(InvalidTransaction::BlobVersionNotSupported)),
                ) => return Ok(()),
                (
                    Some("TransactionException.INSUFFICIENT_ACCOUNT_FUNDS"),
                    Err(EVMError::Transaction(InvalidTransaction::LackOfFundForMaxFee { .. })),
                ) => return Ok(()),
                (
                    Some("TransactionException.TYPE_3_TX_ZERO_BLOBS"),
                    Err(EVMError::Transaction(InvalidTransaction::EmptyBlobs { .. })),
                ) => return Ok(()),
                (
                    Some("TransactionException.INSUFFICIENT_MAX_FEE_PER_BLOB_GAS"),
                    Err(EVMError::Transaction(InvalidTransaction::BlobGasPriceGreaterThanMax {
                        ..
                    })),
                ) => return Ok(()),
                (
                    Some("TransactionException.INSUFFICIENT_MAX_FEE_PER_GAS"),
                    Err(EVMError::Transaction(InvalidTransaction::GasPriceLessThanBasefee {
                        ..
                    })),
                ) => return Ok(()),
                (
                    Some("TR_EMPTYBLOB"),
                    Err(EVMError::Transaction(InvalidTransaction::EmptyBlobs)),
                ) => return Ok(()),
                (Some(_), Err(_)) => todo!(),
                _ => {}
            }

            let res = res.unwrap();

            verify_result(test, unit.out.as_ref(), &res.result)?;
            // TODO: use rlp and hash to check logs
            verify_storage(&test.post_state, res.state);
        }
    }
    Ok(())
}
