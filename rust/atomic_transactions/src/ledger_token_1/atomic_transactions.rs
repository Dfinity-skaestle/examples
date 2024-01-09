use std::{cell::RefCell, collections::BTreeMap};

use candid::Principal;

// A token is always a 3-character string
pub type TokenName = String;
// XXX The transaction ID has to contain the sender principal ID, so that it is unique
pub type TransactionId = usize;

thread_local! {
    // Balances of tokens stored in this ledger
    static PC_STATE: RefCell<BTreeMap<TokenName, TransactionStatus>> = RefCell::new(
        BTreeMap::new());
}

pub(crate) fn with_state<R>(f: impl FnOnce(&BTreeMap<TokenName, TransactionStatus>) -> R) -> R {
    PC_STATE.with_borrow(|pc_state| f(pc_state))
}

pub(crate) fn with_state_mut<R>(
    f: impl FnOnce(&mut BTreeMap<TokenName, TransactionStatus>) -> R,
) -> R {
    PC_STATE.with_borrow_mut(|pc_state| f(pc_state))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransactionState {
    status: TransactionStatus,
    owner: Principal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionStatus {
    Prepared(TransactionId),
    // Need to maintain lists of aborted and committed transactions
    Aborted,
    Comitted,
}

/// Abort the given transaction.
///
/// No action will be executed unless the current state is "Prepared" with the given transaction ID.
pub fn abort_transaction(tid: TransactionId, resource: TokenName) {
    with_state_mut(|state| {
        if state.get(&resource) == Some(&TransactionStatus::Prepared(tid)) {
            state.insert(resource.clone(), TransactionStatus::Aborted);
            ic_cdk::println!(
                "Transaction {} aborted: state was: {:?}",
                tid,
                state.get(&resource)
            );
        } else {
            ic_cdk::println!(
                "Transaction {} not aborted: state is {:?}",
                tid,
                state.get(&resource)
            );
        }
    });
}

/// Generic prepare function.
///
/// Accepts an arbitrary function, which is used to evaluate whether the prepare statement can be accepted.
pub fn prepare_transaction<T>(
    tid: TransactionId,
    resource: TokenName,
    balance_change: T,
    prepare_func: impl FnOnce(&TokenName, T) -> bool,
) -> bool {
    let r = with_state(|state| {
        let current_state = state.get(&resource);
        ic_cdk::println!(
            "Current state of token {:?}: {:?}",
            &resource,
            current_state
        );
        match current_state {
            Some(TransactionStatus::Prepared(prepared_tid)) => {
                // Resource already in prepare state, reject further prepare statements.
                if &tid == prepared_tid {
                    // This is a retry of the same transaction, so we can accept it
                    ic_cdk::println!(
                        "Token already prepared for this transaction {} - accepting prepare statement",
                        tid
                    );
                    true
                } else {
                    // This is a different transaction, so we reject it
                    ic_cdk::println!("Token already prepared for another transaction {} - rejecting prepare statement for {}", prepared_tid, tid);
                    false
                }
            }
            Some(TransactionStatus::Aborted) | Some(TransactionStatus::Comitted) | None => {
                prepare_func(&resource, balance_change)
            }
        }
    });
    if r {
        with_state_mut(|state| {
            state.insert(resource, TransactionStatus::Prepared(tid));
        });
    }
    r
}

/// XXX - This is currently not idempotent.
///
/// For it to be idempotent, we would need to maintain a log of committed transactions.
pub fn commit_transaction<T>(
    tid: TransactionId,
    resource: TokenName,
    balance_change: T,
    commit_func: impl FnOnce(&TokenName, T),
) {
    with_state_mut(|state| {
        assert_eq!(
            state.get(&resource),
            Some(&TransactionStatus::Prepared(tid))
        );
        commit_func(&resource, balance_change);
        state.insert(resource, TransactionStatus::Comitted);
    });
}

pub fn print_state() {
    with_state(|state| {
        ic_cdk::println!("Current state of ledger:");
        for (resource, status) in state.iter() {
            ic_cdk::println!("Resources state:  {:?} {:?}", resource, status);
        }
    });
}
