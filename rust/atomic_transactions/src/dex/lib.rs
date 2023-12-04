use candid::{CandidType, Encode, Principal};
use ic_cdk::api::call::{call, call_raw};
use ic_cdk::api::management_canister::main::{
    create_canister, install_code, CanisterInstallMode, CreateCanisterArgument, InstallCodeArgument,
};
use ic_cdk::api::management_canister::provisional::CanisterSettings;
use ic_cdk_macros::{query, update};

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::RwLock;

type CanisterId = Principal;
type TransactionId = usize;

const NUM_LEDGERS: usize = 5;

// Inline wasm binary of data partition canister
pub const WASM: &[u8] =
    include_bytes!("../../target/wasm32-unknown-unknown/release/ledger_token_1.wasm.gz");

thread_local! {
    // A list of canister IDs for data partitions
    static CANISTER_IDS: Arc<RwLock<Vec<Principal>>> = Arc::new(RwLock::new(vec![]));
    static TRANSACTION_STATE: Arc<RwLock<TransactionList>> = Arc::new(RwLock::new(TransactionList {
        transaction_number: 0,
        transactions: BTreeMap::new(),
    }));
}

#[derive(CandidType, Debug, Copy, Clone)]
enum TransactionStatus {
    Preparing,
    Committed,
    Failed,
}

struct TransactionList {
    transaction_number: TransactionId,
    transactions: BTreeMap<TransactionId, TransactionState>,
}

struct TransactionState {
    num_commit_okay: u64,
    num_commit_fail: u64,
    total_number_of_children: u64, // Total number of canisters participating in 2PC
    pending_commits: Vec<Call>,
    transaction_status: TransactionStatus,
}

#[derive(CandidType, Debug)]
struct TransactionResult {
    transaction_number: TransactionId,
    state: TransactionStatus,
}

struct Call {
    target: CanisterId,
    method: String,
    payload: Vec<u8>, // or similar
}

fn get_canister_ids() -> Vec<CanisterId> {
    CANISTER_IDS.with(|canister_ids| {
        let canister_ids = canister_ids.read().unwrap();
        canister_ids.clone()
    })
}

#[update]
async fn init() {
    // Create partitions if they don't exist yet
    if CANISTER_IDS.with(|canister_ids| {
        let canister_ids = canister_ids.read().unwrap();
        canister_ids.len() == 0
    }) {
        create_ledgers_from_wasm().await;
    }
}

fn get_transaction_state(tid: TransactionId) -> TransactionResult {
    TRANSACTION_STATE.with(|state| {
        let state = state.read().unwrap();
        let transaction_state = state.transactions.get(&tid).unwrap();
        TransactionResult {
            transaction_number: tid,
            state: transaction_state.transaction_status,
        }
    })
}

#[update]
async fn start_transaction() -> TransactionResult {
    TRANSACTION_STATE.with(|state| {
        // Get next possible transaction number
        let mut state = state.write().unwrap();
        state.transaction_number += 1;

        let tid = state.transaction_number;

        state.transactions.insert(
            tid,
            TransactionState {
                num_commit_okay: 0,
                num_commit_fail: 0,
                total_number_of_children: get_canister_ids().len() as u64,
                pending_commits: vec![],
                transaction_status: TransactionStatus::Preparing,
            },
        );

        get_transaction_state(tid)
    })
}

#[update]
/// Resume executing a transaction.
///
/// Calling this function might change the state of the transaction.
async fn check_transaction_state(tid: TransactionId) -> TransactionResult {
    let transaction_state = TRANSACTION_STATE.with(|state| {
        let mut state = state.write().unwrap();
        let transaction_state = state.transactions.get_mut(&tid).unwrap();
        transaction_state
    });

    match transaction_state.transaction_status {
        TransactionStatus::Preparing => {
            // Trigger all calls that have not been triggered yet
            for call in transaction_state.pending_commits.iter() {
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                if call_raw_result.is_ok() {
                    transaction_state.num_commit_okay += 1;
                } else {
                    // Should we retry in this case?
                    transaction_state.num_commit_fail += 1;
                }
            }
        }
        TransactionStatus::Committed => todo!(),
        TransactionStatus::Failed => todo!(),
    }

    get_transaction_state(tid)
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

    for i in 0..NUM_LEDGERS {
        let canister_record = create_canister(create_args.clone()).await.unwrap();
        let canister_id = canister_record.0.canister_id;

        ic_cdk::println!("Created canister {}", canister_id);

        let install_args = InstallCodeArgument {
            mode: CanisterInstallMode::Install,
            canister_id,
            wasm_module: WASM.to_vec(),
            arg: vec![],
        };

        install_code(install_args).await.unwrap();

        let token_names: Vec<u32> = vec![i as u32];
        let token_balances: Vec<u64> = vec![1000];
        let _: () = ic_cdk::call(canister_id, "init", (token_names, token_balances))
            .await
            .unwrap();

        CANISTER_IDS.with(|canister_ids| {
            let mut canister_ids = canister_ids.write().unwrap();
            canister_ids.push(canister_id);
        });
    }
}
