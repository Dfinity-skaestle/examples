use ansi_term::Style;
use atomic_transactions::{TransactionId, TransactionResult, TransactionState};
use candid::{Decode, Encode, Principal};
use ic_cdk::api::call::call_raw;

use ic_cdk::api::management_canister::provisional::CanisterId;
use ic_cdk_macros::{query, update};

use std::{cell::RefCell, time::Duration};

mod atomic_transactions;
mod utils;
use crate::atomic_transactions::{TransactionList, TransactionStatus};

const TOKEN1: &str = "ICP";
const TOKEN2: &str = "USD";

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
    let tid = TRANSACTION_STATE.with(|state| {
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
                    &Encode!(&tid, &token1, &amount1).unwrap(),
                    &Encode!(&tid, &token2, &amount2).unwrap(),
                ],
            ),
        );
        tid
    });

    ic_cdk::println!("Transaction {} initialized", tid);
    with_state(tid, atomic_transactions::get_transaction_state)
}

fn get_active_transactions() -> Vec<TransactionId> {
    TRANSACTION_STATE.with(|state| {
        let state = state.borrow();
        state
            .transactions
            .iter()
            .filter(|(_, state)| {
                state.transaction_status != TransactionStatus::Committed
                    && state.transaction_status != TransactionStatus::Aborted
            })
            .map(|(tid, _)| *tid)
            .collect()
    })
}

/// Transactions can also be driven by timers.
async fn timer_loop() {
    ic_cdk::println!("Timer loop");
    let mut transactions_executed = 0;
    for tid in get_active_transactions() {
        transaction_loop(tid).await;
        transactions_executed += 1;
    }
    if transactions_executed > 0 {
        ic_cdk::println!(
            "{}",
            Style::new().fg(ansi_term::Color::Green).paint(format!(
                "Timer loop - {} transactions triggered",
                transactions_executed
            ))
        );
    } else {
        ic_cdk::println!("Timer loop - no transactions");
    }
    // XXX Optimization: Schedule timer not every 1 second, but based on the state of active transactions.
    ic_cdk_timers::set_timer(Duration::from_secs(1), || ic_cdk::spawn(timer_loop()));
}

#[update]
/// Resume executing a transaction.
///
/// Calling this function might change the state of the transaction.
/// This can either be triggered peridocially by the user or by a timer.
///
/// Returns the state of the transaction.
async fn transaction_loop(tid: TransactionId) -> TransactionResult {
    let initial_transaction_status = with_state(tid, atomic_transactions::get_transaction_status);
    ic_cdk::println!(
        "Executing transaction {} with status {:?}",
        tid,
        initial_transaction_status
    );

    const TIMEOUT: u64 = 5 * 1000 * 1000 * 1000;
    if ic_cdk::api::time() <= with_state(tid, |_, s| s.last_action_time) + TIMEOUT {
        ic_cdk::println!("Rate limiting transaction {}", tid);
        return with_state(tid, atomic_transactions::get_transaction_state);
    }

    match initial_transaction_status {
        TransactionStatus::Preparing => {
            let pending_prepare_calls = with_state(tid, |_, f| f.pending_prepare_calls.clone());

            // Trigger all calls that have not been triggered yet
            for call in pending_prepare_calls {
                // Nothing to do if we already have a successful call
                if call.num_success > 0 {
                    continue;
                }

                ic_cdk::println!(
                    "Calling {} with method {} and payload {:?}",
                    call.target,
                    call.method,
                    call.payload
                );

                with_state_mut(tid, |_, s| s.last_action_time = ic_cdk::api::time());

                with_state_mut(tid, |_, s| s.register_prepare_call(call.target.clone()));
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                with_state_mut(tid, |_, s| {
                    let style = if call_raw_result.is_ok() {
                        Style::new().bold().fg(ansi_term::Color::Green)
                    } else {
                        Style::new().bold().fg(ansi_term::Color::Red)
                    };
                    ic_cdk::println!(
                        "{}",
                        style.paint(format!("Call result: {:?}", call_raw_result))
                    );
                    let succ = match call_raw_result {
                        Ok(payload) => {
                            let successful_prepare: bool = Decode!(&payload, bool).unwrap();
                            ic_cdk::println!("Received prepare response: {}", successful_prepare);
                            successful_prepare
                        }
                        Err(_) => false,
                    };
                    s.prepare_received(succ, call.target)
                });
            }
        }
        TransactionStatus::Aborting => {
            let pending_abort_calls = with_state(tid, |_, f| f.pending_abort_calls.clone());

            // Trigger all calls that have not been triggered yet
            for call in pending_abort_calls {
                // Nothing to do if we already have a successful call
                if call.num_success > 0 {
                    continue;
                }

                ic_cdk::println!(
                    "Calling {} with method {} and payload {:?}",
                    call.target,
                    call.method,
                    call.payload
                );

                with_state_mut(tid, |_, s| s.last_action_time = ic_cdk::api::time());

                with_state_mut(tid, |_, s| s.register_abort_call(call.target.clone()));
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                with_state_mut(tid, |_, s| {
                    s.abort_received(call_raw_result.is_ok(), call.target)
                });
            }
        }
        TransactionStatus::Committing => {
            let pending_commit_calls = with_state_mut(tid, |_, f| f.pending_commit_calls.clone());

            // Trigger all calls that have not been triggered yet
            for call in pending_commit_calls {
                // Nothing to do if we already have a successful call
                if call.num_success > 0 {
                    continue;
                }

                ic_cdk::println!(
                    "Calling {} with method {} and payload {:?}",
                    call.target,
                    call.method,
                    call.payload
                );

                with_state_mut(tid, |_, s| s.last_action_time = ic_cdk::api::time());

                with_state_mut(tid, |_, s| s.register_commit_call(call.target.clone()));
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                with_state_mut(tid, |_, s| {
                    s.commit_received(call_raw_result.is_ok(), call.target)
                });
            }
        }
        // We are already in a final state, no need to do anything
        TransactionStatus::Committed => {}
        TransactionStatus::Aborted => {}
    }

    let new_transaction_status = with_state(tid, |_, state| {
        ic_cdk::println!("Transaction {} state is: {:?}", tid, state);
        state.transaction_status
    });

    if new_transaction_status != initial_transaction_status {
        let o = format!(
            "Transaction {} state changed from {:?} to {:?}",
            tid, initial_transaction_status, new_transaction_status
        );
        ic_cdk::println!("{}", Style::new().fg(ansi_term::Color::Yellow).paint(o));
        // Reset last action time, so that the next action on this transaction can be executed immediately.
        with_state_mut(tid, |_, s| s.last_action_time = 0);
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

    ic_cdk_timers::set_timer(Duration::from_secs(1), || {
        ic_cdk::spawn(timer_loop());
    });
}

fn get_canister_ids() -> Vec<CanisterId> {
    CANISTER_IDS.with(|canister_ids| canister_ids.borrow().clone())
}

// https://github.com/dfinity/ic-docutrack/blob/main/backend/src/lib.rs#L222

/// A helper method to mutate the state.
pub(crate) fn with_state_mut<R>(
    tid: TransactionId,
    f: impl FnOnce(TransactionId, &mut TransactionState) -> R,
) -> R {
    TRANSACTION_STATE.with(|cell| f(tid, cell.borrow_mut().transactions.get_mut(&tid).unwrap()))
}

/// A helper method to access the state.
pub(crate) fn with_state<R>(
    tid: TransactionId,
    f: impl FnOnce(TransactionId, &TransactionState) -> R,
) -> R {
    TRANSACTION_STATE.with(|cell| f(tid, cell.borrow_mut().transactions.get(&tid).unwrap()))
}
