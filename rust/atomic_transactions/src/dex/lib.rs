use candid::{CandidType, Encode, Principal};
use ic_cdk::api::call::{call, call_raw};
use ic_cdk::api::management_canister::main::{
    create_canister, install_code, CanisterInstallMode, CreateCanisterArgument, InstallCodeArgument,
};
use ic_cdk::api::management_canister::provisional::CanisterSettings;
use ic_cdk_macros::{query, update};

use std::cell::RefCell;
use std::collections::BTreeMap;

type CanisterId = Principal;
type TransactionId = usize;

const NUM_LEDGERS: usize = 5;

// Inline wasm binary of data partition canister
pub const WASM: &[u8] =
    include_bytes!("../../target/wasm32-unknown-unknown/release/ledger_token_1.wasm.gz");

thread_local! {
    // A list of canister IDs for data partitions
    static CANISTER_IDS: RefCell<Vec<Principal>> = RefCell::new(vec![]);
    static TRANSACTION_STATE: RefCell<TransactionList> = RefCell::new(TransactionList {
        next_transaction_number: 0,
        transactions: BTreeMap::new(),
    });
}

#[derive(CandidType, Debug, Copy, Clone, PartialEq, Eq)]
enum TransactionStatus {
    // Active states
    Preparing,
    Aborting,
    Committing,
    // Final states
    Committed,
    Aborted,
}

struct TransactionList {
    next_transaction_number: TransactionId,
    transactions: BTreeMap<TransactionId, TransactionState>,
}

struct TransactionState {
    total_number_of_children: u64, // Total number of canisters participating in 2PC
    transaction_status: TransactionStatus,
    // Prepare
    pending_prepare_calls: Vec<Call>,
    num_prepare_okay: u64,
    num_prepare_fail: u64,
    // Abort
    pending_abort_calls: Vec<Call>,
    num_abort_okay: u64,
    num_abort_fail: u64,
    // Commit
    pending_commit_calls: Vec<Call>,
    num_commit_okay: u64,
    num_commit_fail: u64,
}

#[derive(CandidType, Debug)]
struct TransactionResult {
    transaction_number: TransactionId,
    state: TransactionStatus,
}

#[derive(Clone)]
struct Call {
    target: CanisterId,
    method: String,
    payload: Vec<u8>,
}

fn get_canister_ids() -> Vec<CanisterId> {
    CANISTER_IDS.with(|canister_ids| canister_ids.borrow().clone())
}

// https://github.com/dfinity/ic-docutrack/blob/main/backend/src/lib.rs#L222

#[update]
async fn init() {
    // Create partitions if they don't exist yet
    if CANISTER_IDS.with(|canister_ids| {
        let canister_ids = canister_ids.borrow();
        canister_ids.len() == 0
    }) {
        create_ledgers_from_wasm().await;
    }
}

fn _get_transaction_state(tid: TransactionId) -> TransactionResult {
    TRANSACTION_STATE.with(|state| {
        let state = state.borrow();
        let transaction_state = state.transactions.get(&tid).unwrap();
        TransactionResult {
            transaction_number: tid,
            state: transaction_state.transaction_status,
        }
    })
}

#[update]
async fn swap_token1_to_token2() -> TransactionResult {
    TRANSACTION_STATE.with(|state| {
        // Get next possible transaction number
        let mut state = state.borrow_mut();
        state.next_transaction_number += 1;

        let tid = state.next_transaction_number;

        let canisters = get_canister_ids();
        let canister_id_1 = canisters[0];
        let canister_id_2 = canisters[1];

        state.transactions.insert(
            tid,
            TransactionState {
                total_number_of_children: get_canister_ids().len() as u64,
                transaction_status: TransactionStatus::Preparing,
                // Prepare
                pending_prepare_calls: vec![
                    Call {
                        target: canister_id_1,
                        method: "prepare_transaction".to_string(),
                        payload: Encode!(&(tid, 1, -1337,)).unwrap(),
                    },
                    Call {
                        target: canister_id_2,
                        method: "prepare_transaction".to_string(),
                        payload: Encode!(&(tid, 2, 42,)).unwrap(),
                    },
                ],
                num_prepare_okay: 0,
                num_prepare_fail: 0,
                // Abort
                pending_abort_calls: vec![
                    Call {
                        target: canister_id_1,
                        method: "abort_transaction".to_string(),
                        payload: Encode!(&(tid, 1, -1337,)).unwrap(),
                    },
                    Call {
                        target: canister_id_2,
                        method: "abort_transaction".to_string(),
                        payload: Encode!(&(tid, 2, 42,)).unwrap(),
                    },
                ],
                num_abort_okay: 0,
                num_abort_fail: 0,
                // Commit
                pending_commit_calls: vec![
                    Call {
                        target: canister_id_1,
                        method: "commit_transaction".to_string(),
                        payload: Encode!(&(tid, 1, -1337,)).unwrap(),
                    },
                    Call {
                        target: canister_id_2,
                        method: "commit_transaction".to_string(),
                        payload: Encode!(&(tid, 2, 42,)).unwrap(),
                    },
                ],
                num_commit_okay: 0,
                num_commit_fail: 0,
            },
        );

        _get_transaction_state(tid)
    })
}

#[query]
fn get_transaction_state(tid: TransactionId) -> TransactionResult {
    _get_transaction_state(tid)
}

#[update]
/// Resume executing a transaction.
///
/// Calling this function might change the state of the transaction.
async fn transaction_loop(tid: TransactionId) -> TransactionResult {
    let transaction_status = TRANSACTION_STATE.with(|state| {
        state
            .borrow()
            .transactions
            .get(&tid)
            .unwrap()
            .transaction_status
    });

    match transaction_status {
        TransactionStatus::Preparing => {
            let pending_prepare_calls = TRANSACTION_STATE.with(|state| {
                state
                    .borrow()
                    .transactions
                    .get(&tid)
                    .unwrap()
                    .pending_prepare_calls
                    .clone()
            });

            // Trigger all calls that have not been triggered yet
            for call in pending_prepare_calls {
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                TRANSACTION_STATE.with(|state| {
                    let mut state = state.borrow_mut();
                    let transaction_state = state.transactions.get_mut(&tid).unwrap();
                    if call_raw_result.is_ok() {
                        transaction_state.num_prepare_okay += 1;
                    } else {
                        transaction_state.num_prepare_fail += 1;
                    }
                });
            }

            TRANSACTION_STATE.with(|state| {
                let mut state = state.borrow_mut();
                let transaction_state = state.transactions.get_mut(&tid).unwrap();

                // Change the state of the transaction based on the total number of responses
                if transaction_state.num_prepare_okay == transaction_state.total_number_of_children
                    && transaction_state.transaction_status == TransactionStatus::Preparing
                {
                    transaction_state.transaction_status = TransactionStatus::Committing;
                } else if transaction_state.num_prepare_fail > 0 {
                    transaction_state.transaction_status = TransactionStatus::Aborting;
                }
            });
        }
        TransactionStatus::Aborting => {
            let pending_abort_calls = TRANSACTION_STATE.with(|state| {
                state
                    .borrow()
                    .transactions
                    .get(&tid)
                    .unwrap()
                    .pending_abort_calls
                    .clone()
            });

            // Trigger all calls that have not been triggered yet
            for call in pending_abort_calls {
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                TRANSACTION_STATE.with(|state| {
                    let mut state = state.borrow_mut();
                    let transaction_state = state.transactions.get_mut(&tid).unwrap();
                    if call_raw_result.is_ok() {
                        transaction_state.num_abort_okay += 1;
                    } else {
                        transaction_state.num_abort_fail += 1;
                    }
                });
            }

            TRANSACTION_STATE.with(|state| {
                let mut state = state.borrow_mut();
                let transaction_state = state.transactions.get_mut(&tid).unwrap();

                // Change the state of the transaction based on the total number of responses
                if transaction_state.num_abort_okay == transaction_state.total_number_of_children {
                    transaction_state.transaction_status = TransactionStatus::Aborted;
                } else if transaction_state.num_abort_fail > 0 {
                    // No state change here!
                }
            });
        }
        TransactionStatus::Committing => {
            let pending_commit_calls = TRANSACTION_STATE.with(|state| {
                state
                    .borrow()
                    .transactions
                    .get(&tid)
                    .unwrap()
                    .pending_commit_calls
                    .clone()
            });

            // Trigger all calls that have not been triggered yet
            for call in pending_commit_calls {
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                TRANSACTION_STATE.with(|state| {
                    let mut state = state.borrow_mut();
                    let transaction_state = state.transactions.get_mut(&tid).unwrap();
                    if call_raw_result.is_ok() {
                        transaction_state.num_commit_okay += 1;
                    } else {
                        transaction_state.num_commit_fail += 1;
                    }
                });
            }

            TRANSACTION_STATE.with(|state| {
                let mut state = state.borrow_mut();
                let transaction_state = state.transactions.get_mut(&tid).unwrap();

                // Change the state of the transaction based on the total number of responses
                if transaction_state.num_commit_okay == transaction_state.total_number_of_children {
                    transaction_state.transaction_status = TransactionStatus::Committed;
                } else if transaction_state.num_commit_fail > 0 {
                    // No state change here!
                }
            });
        }
        // We are already in a final state, no need to do anything
        TransactionStatus::Committed => {}
        TransactionStatus::Aborted => {}
    }

    _get_transaction_state(tid)
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

        let token_names: Vec<u32> = vec![(i + 1) as u32];
        let token_balances: Vec<u64> = vec![10000];
        let _: () = ic_cdk::call(canister_id, "init", (token_names, token_balances))
            .await
            .unwrap();

        CANISTER_IDS.with(|canister_ids| {
            let mut canister_ids = canister_ids.borrow_mut();
            canister_ids.push(canister_id);
        });
    }
}
