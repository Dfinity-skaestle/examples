use candid::Principal;
use ic_cdk::api::call::call;
use ic_cdk::api::management_canister::main::{
    create_canister, install_code, CanisterInstallMode, CreateCanisterArgument, InstallCodeArgument,
};
use ic_cdk::api::management_canister::provisional::CanisterSettings;
use ic_cdk_macros::{query, update};

use std::sync::Arc;
use std::sync::RwLock;

const NUM_PARTITIONS: usize = 5;

// Inline wasm binary of data partition canister
pub const WASM: &[u8] = (
    include_bytes!("../../target/wasm32-unknown-unknown/release/ledger_token_1.wasm.gz"),
    include_bytes!("../../target/wasm32-unknown-unknown/release/ledger_token_2.wasm.gz"),
);

thread_local! {
    // A list of canister IDs for data partitions
    static CANISTER_IDS: Arc<RwLock<Vec<Principal>>> = Arc::new(RwLock::new(vec![]));
}

#[update]
async fn init() {
    // Create partitions if they don't exist yet
    if CANISTER_IDS.with(|canister_ids| {
        let canister_ids = canister_ids.read().unwrap();
        canister_ids.len() == 0
    }) {
        for _ in 0..NUM_PARTITIONS {
            create_ledgers_from_wasm().await;
        }
    }
}

async fn create_ledgers_from_wasm() {
    let create_args = CreateCanisterArgument {
        settings: Some(CanisterSettings {
            controllers: Some(vec![ic_cdk::id()]),
            compute_allocation: Some(0.into()),
            memory_allocation: Some(0.into()),
            freezing_threshold: Some(0.into()),
        }),
    };

    // Create ledger 1
    let canister_record = create_canister(create_args).await.unwrap();
    let canister_id = canister_record.0.canister_id;

    ic_cdk::println!("Created canister {}", canister_id);

    let install_args = InstallCodeArgument {
        mode: CanisterInstallMode::Install,
        canister_id,
        wasm_module: WASM.0.to_vec(),
        arg: vec![],
    };

    install_code(install_args).await.unwrap();

    CANISTER_IDS.with(|canister_ids| {
        let mut canister_ids = canister_ids.write().unwrap();
        canister_ids.push(canister_id);
    });

    // Create ledger 2
    let canister_record = create_canister(create_args).await.unwrap();
    let canister_id = canister_record.0.canister_id;

    ic_cdk::println!("Created canister {}", canister_id);

    let install_args = InstallCodeArgument {
        mode: CanisterInstallMode::Install,
        canister_id,
        wasm_module: WASM.1.to_vec(),
        arg: vec![],
    };

    install_code(install_args).await.unwrap();

    CANISTER_IDS.with(|canister_ids| {
        let mut canister_ids = canister_ids.write().unwrap();
        canister_ids.push(canister_id);
    });
}
