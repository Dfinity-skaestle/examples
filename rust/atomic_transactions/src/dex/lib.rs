use atomic_transactions::{TransactionResult, TransactionState};
use candid::{Encode, Principal};

use ic_cdk_macros::update;

use std::cell::RefCell;

mod atomic_transactions;
mod utils;

thread_local! {
    // A list of canister IDs for data partitions
    static CANISTER_IDS: RefCell<Vec<Principal>> = RefCell::new(vec![]);
}

fn with_canisters<R>(f: impl FnOnce(&mut Vec<Principal>) -> R) -> R {
    CANISTER_IDS.with(|canister_ids| f(&mut canister_ids.borrow_mut()))
}

#[update]
/// Initialize transaction that executes an arbitrary token swap.
async fn swap_tokens(
    token1: String,
    token2: String,
    amount1: i64,
    amount2: i64,
) -> TransactionResult {
    let (canister_1, canister_2) = with_canisters(|canisters| (canisters[0], canisters[1]));

    // Allocate a transaction number
    let tid = atomic_transactions::get_next_transaction_number();
    ic_cdk::println!("Transaction {} initialized", tid);

    // Setup the transaction
    // This basically entails registering the methods that should be called by the transaction logic.
    atomic_transactions::add_transaction(
        TransactionState::new(
            &[canister_1, canister_2],
            "prepare_transaction",
            "abort_transaction",
            "commit_transaction",
            &[
                &Encode!(&tid, &token1, &amount1).unwrap(),
                &Encode!(&tid, &token2, &amount2).unwrap(),
            ],
        ),
        tid,
    )
}

#[update]
/// Initialize the "ledgers" used in this demo.
///
/// This code has to stay here (vs in the library) because it isn't specific to the actual
/// transactions. It basically sets up the user-specific part of the logic.
async fn init() {
    ic_cdk::println!("---------------------");

    // Install ledgers, if not already done.
    if !has_canisters() {
        let principals = utils::create_ledgers_from_wasm().await;
        with_canisters(|canisters| canisters.extend(principals));
    }

    // Initialize the atomic transaction library
    atomic_transactions::init();
}

fn has_canisters() -> bool {
    with_canisters(|canisters| canisters.len() > 0)
}
