use candid::Principal;
use ic_cdk_macros::update;
use std::{cell::RefCell, collections::BTreeMap};

// A token is always a 3-character string
type TokenName = String;

type TokenBalance = u64;
type TransactionId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransactionState {
    status: TransactionStatus,
    owner: Principal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransactionStatus {
    Prepared(TransactionId),
    Aborted,
    Comitted,
}

thread_local! {
    // Balances of tokens stored in this ledger
    static BALANCES: RefCell<BTreeMap<TokenName, TokenBalance>> = RefCell::new(
        BTreeMap::new());

    // Balances of tokens stored in this ledger
    static PC_STATE: RefCell<BTreeMap<TokenName, TransactionStatus>> = RefCell::new(
        BTreeMap::new());
}

#[update]
/// Prepare initial balances of this ledger
fn init(token_names: Vec<TokenName>, token_balances: Vec<TokenBalance>) {
    BALANCES.with_borrow_mut(|balances| {
        for (name, balance) in token_names.iter().zip(token_balances) {
            balances.insert(name.clone(), balance);
            ic_cdk::println!("Ledger: Inital token {:?} with balance {}", name, balance);
        }
    });
}

#[update]
/// Prepare atomic transactions by means of Two-Phase Commit
///
/// This code ensures that resource exists and that the change in balance does not create overflows.
/// It also ensures that the given resource has not already been prepared by another transaction.
/// If this is okay, response "yes", otherwise "no".
///
/// XXX - This is currently not idempotent. For that, we would need to record the principal for which
/// a previous prepare has been issued.
fn prepare_transaction(tid: TransactionId, resource: TokenName, balance_change: i64) -> bool {
    ic_cdk::println!("Preparing transaction: {} for resource {:?}", tid, resource);

    let r = PC_STATE.with_borrow_mut(|pc_state| {
        BALANCES.with_borrow_mut(|balances| {
            let current_state = pc_state.get(&resource);
            ic_cdk::println!("Current state of token {:?}: {:?}", resource, current_state);
            match current_state {
                Some(TransactionStatus::Prepared(tid)) => {
                    // Resource already in prepare state, reject further prepare statements.
                    ic_cdk::println!(
                        "Token already prepared for {} - rejecting prepare statement",
                        tid
                    );
                    false
                }
                Some(TransactionStatus::Aborted) | Some(TransactionStatus::Comitted) | None => {
                    match balances.get(&resource) {
                        Some(resource_balance) => {
                            // Check if given balance exists and if overflow would happen for the given change in balance
                            match resource_balance.checked_add_signed(balance_change) {
                                Some(_) => {
                                    // Resource not locked in 2PC
                                    pc_state.insert(resource, TransactionStatus::Prepared(tid));
                                    ic_cdk::println!(
                                        "Token prepared - accepting prepare statement"
                                    );
                                    true
                                }

                                None => {
                                    ic_cdk::println!(
                                        "Token balance overflow - rejecting prepare statement"
                                    );
                                    false
                                }
                            }
                        }
                        None => {
                            ic_cdk::println!("Token does not exist - rejecting prepare statement");
                            false
                        }
                    }
                }
            }
        })
    });
    print_state();
    r
}

#[update]
/// Abort previously prepared transaction.
///
/// Resets the state of the given resource to "aborted". This will free up resources that have
/// previously been locked by responding "yes" to previous "prepare" requests.
///
/// Aborting of the transaction is unconditional.
///
/// Has to be idempotent.
fn abort_transaction(tid: TransactionId, resource: TokenName, _balance_change: i64) {
    ic_cdk::println!("Aborting transaction: {} for resource {:?}", tid, resource);
    let r = PC_STATE.with_borrow_mut(|pc_state| {
        if pc_state.get(&resource) == Some(&TransactionStatus::Prepared(tid)) {
            pc_state.insert(resource.clone(), TransactionStatus::Aborted);
            ic_cdk::println!(
                "Transaction {} aborted: state was: {:?}",
                tid,
                pc_state.get(&resource)
            );
        } else {
            ic_cdk::println!(
                "Transaction {} not aborted: state is {:?}",
                tid,
                pc_state.get(&resource)
            );
        }
    });
    print_state();
    r
}

#[update]
/// Commit changes according to previously prepared balance change and resource.
///
/// If this fails, there is likely a bug in the protocol.
///
/// XXX - This is currently not idempotent.
fn commit_transaction(tid: TransactionId, resource: TokenName, balance_change: i64) {
    ic_cdk::println!("Committing transaction: {} for token {:?}", tid, resource);
    PC_STATE.with_borrow_mut(|pc_state| {
        assert_eq!(
            pc_state.get(&resource),
            Some(&TransactionStatus::Prepared(tid))
        );
        BALANCES.with_borrow_mut(|balances| {
            balances.insert(
                resource.clone(),
                balances
                    .get(&resource)
                    .expect("Token does not have a registered balance - prepare should have failed")
                    .checked_add_signed(balance_change)
                    .expect("Token balance overflow - prepare should have failed"),
            );
            pc_state.insert(resource, TransactionStatus::Comitted);
        });
    });
    print_state();
}

fn print_state() {
    PC_STATE.with_borrow(|pc_state| {
        for (token, status) in pc_state.iter() {
            ic_cdk::println!("Token state: {:?} {:?}", token, status);
        }
    });

    BALANCES.with_borrow(|balances| {
        for (token, balance) in balances.iter() {
            ic_cdk::println!("Token balance: {:?} {:?}", token, balance);
        }
    });
}
