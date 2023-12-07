use atomic_transactions::{TransactionId, TransactionResult, TransactionState};
use candid::{Encode, Principal};
use ic_cdk::api::call::call_raw;

use ic_cdk::api::management_canister::provisional::CanisterId;
use ic_cdk_macros::{query, update};

use std::cell::RefCell;

mod atomic_transactions;
mod utils;
use crate::atomic_transactions::{TransactionList, TransactionStatus};

const TOKEN1: u32 = 1;
const TOKEN2: u32 = 2;

thread_local! {
    // A list of canister IDs for data partitions
    static CANISTER_IDS: RefCell<Vec<Principal>> = RefCell::new(vec![]);
    static TRANSACTION_STATE: RefCell<TransactionList> = RefCell::new(TransactionList::default());
}

#[update]
/// Initialize transaction that executes the token swap.
///
/// Executes a hypothetical token swap between two tokens, where 1337 units of token 1 are swapped for 42 tokens of token 2.
async fn swap_token1_to_token2() -> TransactionResult {
    TRANSACTION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let tid = state.get_next_transaction_number();

        let canisters = get_canister_ids();
        let canister_id_1 = canisters[0];
        let canister_id_2 = canisters[1];

        state.transactions.insert(
            tid,
            TransactionState::new(
                &[canister_id_1, canister_id_2],
                "prepare_transaction",
                "abort_transaction",
                "commit_transaction",
                &[
                    &Encode!(&(tid, TOKEN1, -1337,)).unwrap(),
                    &Encode!(&(tid, TOKEN2, 42,)).unwrap(),
                ],
            ),
        );

        with_state(tid, atomic_transactions::get_transaction_state)
    })
}

#[update]
/// Resume executing a transaction.
///
/// Calling this function might change the state of the transaction.
/// This can either be triggered peridocially by the user or by a timer.
///
/// Returns the state of the transaction.
async fn transaction_loop(tid: TransactionId) -> TransactionResult {
    let transaction_status = with_state(tid, atomic_transactions::get_transaction_status);

    match transaction_status {
        TransactionStatus::Preparing => {
            let pending_prepare_calls = with_state(tid, |_, f| f.pending_prepare_calls.clone());

            // Trigger all calls that have not been triggered yet
            for call in pending_prepare_calls {
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                with_state_mut(tid, |_, s| s.prepare_received(call_raw_result.is_ok()));
            }
        }
        TransactionStatus::Aborting => {
            let pending_abort_calls = with_state(tid, |_, f| f.pending_abort_calls.clone());

            // Trigger all calls that have not been triggered yet
            for call in pending_abort_calls {
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                with_state_mut(tid, |_, s| s.abort_received(call_raw_result.is_ok()));
            }
        }
        TransactionStatus::Committing => {
            let pending_commit_calls = with_state_mut(tid, |_, f| f.pending_commit_calls.clone());

            // Trigger all calls that have not been triggered yet
            for call in pending_commit_calls {
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                with_state_mut(tid, |_, s| s.commit_received(call_raw_result.is_ok()));
            }
        }
        // We are already in a final state, no need to do anything
        TransactionStatus::Committed => {}
        TransactionStatus::Aborted => {}
    }

    with_state(tid, atomic_transactions::get_transaction_state)
}

#[query]
/// Get the current state of a transaction.
fn get_transaction_state(tid: TransactionId) -> TransactionResult {
    with_state(tid, atomic_transactions::get_transaction_state)
}

#[update]
/// Initialize the "ledgers" used in this demo.
async fn init() {
    if CANISTER_IDS.with(|canister_ids| {
        let canister_ids = canister_ids.borrow();
        canister_ids.len() == 0
    }) {
        utils::create_ledgers_from_wasm().await;
    }
}

fn get_canister_ids() -> Vec<CanisterId> {
    CANISTER_IDS.with(|canister_ids| canister_ids.borrow().clone())
}

// https://github.com/dfinity/ic-docutrack/blob/main/backend/src/lib.rs#L222

/// A helper method to mutate the state.
pub fn with_state_mut<R>(
    tid: TransactionId,
    f: impl FnOnce(TransactionId, &mut TransactionState) -> R,
) -> R {
    TRANSACTION_STATE.with(|cell| f(tid, cell.borrow_mut().transactions.get_mut(&tid).unwrap()))
}

/// A helper method to access the state.
pub fn with_state<R>(
    tid: TransactionId,
    f: impl FnOnce(TransactionId, &TransactionState) -> R,
) -> R {
    TRANSACTION_STATE.with(|cell| f(tid, cell.borrow_mut().transactions.get(&tid).unwrap()))
}
