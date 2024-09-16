use std::path::Path;
mod ef_tests_executor;
use ef_tests_executor::{
    models::TestSuite,
    test_utils::{setup_evm, verify_result, verify_storage},
};
use evm_mlir::env::TransactTo;

fn run_test(path: &Path, contents: String) -> datatest_stable::Result<()> {
    let test_suite: TestSuite = serde_json::from_reader(contents.as_bytes())
        .unwrap_or_else(|_| panic!("Failed to parse JSON test {}", path.display()));

    for (_name, unit) in test_suite.0 {
        // NOTE: currently we only support Cancun spec
        let Some(tests) = unit.post.get("Cancun") else {
            continue;
        };
        let to = match unit.transaction.to {
            Some(to) => TransactTo::Call(to),
            None => TransactTo::Create,
        };
        let sender = unit.transaction.sender.unwrap_or_default();
        let gas_price = unit.transaction.gas_price.unwrap_or_default();

        for test in tests {
            let mut evm = setup_evm(test, &unit, &to, sender, gas_price);
            let res = evm.transact().unwrap();
            verify_result(test, unit.out.as_ref(), &res.result)?;
            // TODO: use rlp and hash to check logs
            verify_storage(&test.post_state, res.state);
        }
    }
    Ok(())
}

datatest_stable::harness!(
    run_test,
    "ethtests/GeneralStateTests/stTimeConsuming/",
    r"^.*/*.json",
);
