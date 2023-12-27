use atomic_transactions::{TransactionId, TransactionResult, TransactionState};
use candid::{Encode, Principal};

use ic_cdk::api::management_canister::provisional::CanisterId;
use ic_cdk_macros::{query, update};

use std::cell::RefCell;

mod atomic_transactions;
mod utils;

const TOKEN1: &str = "ICP";
const TOKEN2: &str = "USD";

thread_local! {
    // A list of canister IDs for data partitions
    static CANISTER_IDS: RefCell<Vec<Principal>> = RefCell::new(vec![]);
}

#[update]
/// Initialize transaction that executes the token swap.
///
/// Executes a hypothetical token swap between two tokens, where 1337 units of token 1 are swapped for 42 tokens of token 2.
async fn swap_token1_to_token2() -> TransactionResult {
    swap_tokens(TOKEN1.to_string(), TOKEN2.to_string(), -1337, 42).await
}

#[update]
/// Initialize transaction that executes an arbitrary token swap.
async fn swap_tokens(
    token1: String,
    token2: String,
    amount1: i64,
    amount2: i64,
) -> TransactionResult {
    let tid = atomic_transactions::get_next_transaction_number();

    let canisters = get_canister_ids();
    let canister_id_1 = canisters[0];
    let canister_id_2 = canisters[1];

    let new_transaction = TransactionState::new(
        &[canister_id_1, canister_id_2],
        "prepare_transaction",
        "abort_transaction",
        "commit_transaction",
        &[
            &Encode!(&tid, &token1, &amount1).unwrap(),
            &Encode!(&tid, &token2, &amount2).unwrap(),
        ],
    );

    ic_cdk::println!("Transaction {} initialized", tid);
    atomic_transactions::add_transaction(new_transaction, tid)
}

#[update]
/// Disable the timer loop.
/// This is useful for testing.
fn disable_timer(disable: bool) {
    atomic_transactions::disable_timer(disable);
}

#[update]
/// Resume executing a transaction.
///
/// Calling this function might change the state of the transaction.
/// This can either be triggered peridocially by the user or by a timer.
///
/// Returns the state of the transaction.
async fn transaction_loop(tid: TransactionId) -> TransactionResult {
    atomic_transactions::transaction_loop(tid).await
}

#[query]
/// Get the current state of a transaction.
fn get_transaction_state(tid: TransactionId) -> TransactionResult {
    atomic_transactions::get_transaction_state(tid)
}

#[update]
/// Initialize the "ledgers" used in this demo.
async fn init() {
    ic_cdk::println!("---------------------");
    if CANISTER_IDS.with(|canister_ids| {
        let canister_ids = canister_ids.borrow();
        canister_ids.len() == 0
    }) {
        let principals = utils::create_ledgers_from_wasm().await;
        CANISTER_IDS.with(|canister_ids| {
            let mut canister_ids = canister_ids.borrow_mut();
            canister_ids.extend(principals);
        });
    }

    atomic_transactions::init();
}

fn get_canister_ids() -> Vec<CanisterId> {
    CANISTER_IDS.with(|canister_ids| canister_ids.borrow().clone())
}
