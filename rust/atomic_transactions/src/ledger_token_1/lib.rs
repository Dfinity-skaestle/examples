use atomic_transactions::TransactionId;
use ic_cdk_macros::update;
use std::{cell::RefCell, collections::BTreeMap};

type TokenBalance = u64;

mod atomic_transactions;
use crate::atomic_transactions::TokenName;

thread_local! {
    /// Balances of tokens stored in this ledger
    ///
    /// This part is application specific. Other applications might need different state here.
    /// Atomic transactions on it are executed by means of callbacks, which are then
    /// being called by the atomic transactions library in the appropriate places.
    ///
    /// Access to this data structures is mediated by the atomic transaction library.
    /// All modifications to this data structure are unsafe, unless triggered by the
    /// atomic transaction library.
    static BALANCES: RefCell<BTreeMap<TokenName, TokenBalance>> = RefCell::new(
        BTreeMap::new());
}

pub(crate) fn with_balances_mut<R>(
    f: impl FnOnce(&mut BTreeMap<TokenName, TokenBalance>) -> R,
) -> R {
    BALANCES.with_borrow_mut(|balances| f(balances))
}

pub(crate) fn with_balances<R>(f: impl FnOnce(&BTreeMap<TokenName, TokenBalance>) -> R) -> R {
    BALANCES.with_borrow(|balances| f(balances))
}

/// Method to check if the prepare statement can be accepted.
pub fn prepare_balance(resource: &TokenName, balance_change: i64) -> bool {
    // Note: Immutable access to balances here. No modifications to the
    // state are allowed here. Changes are safe to do only from the commit_balance function.
    with_balances(|balances| {
        match balances.get(resource) {
            Some(resource_balance) => {
                // Requested token does exist in ledger.
                // Check if given balance exists and if overflow would happen for the given change in balance
                match resource_balance.checked_add_signed(balance_change) {
                    Some(_) => {
                        ic_cdk::println!("Token prepared - accepting prepare statement");
                        true
                    }

                    None => {
                        ic_cdk::println!("Token balance overflow - rejecting prepare statement");
                        false
                    }
                }
            }
            None => {
                // Requested token does not exist in ledger
                ic_cdk::println!("Token does not exist - rejecting prepare statement");
                false
            }
        }
    })
}

/// Commit the given transaction.
///
/// This method is going to be called by the atomic transaction library once it is safe to
/// commit the requested transaction.
pub fn commit_balance(resource: &TokenName, balance_change: i64) {
    with_balances_mut(|balances| {
        balances.insert(
            resource.clone(),
            balances
                .get(resource)
                .expect("Token does not have a registered balance - prepare should have failed")
                .checked_add_signed(balance_change)
                .expect("Token balance overflow - prepare should have failed"),
        );
    });
}

#[update]
/// Prepare atomic transactions by means of Two-Phase Commit
///
/// This code ensures that resource exists and that the change in balance does not create overflows.
/// It also ensures that the given resource has not already been prepared by another transaction.
/// If this is okay, response "yes", otherwise "no".
///
/// Function is idempotent. If prepared is called multiple times for the same transaction, "true" will be returned.
fn prepare_transaction(tid: TransactionId, resource: TokenName, balance_change: i64) -> bool {
    ic_cdk::println!("Preparing transaction: {} for resource {:?}", tid, resource);
    let r = crate::atomic_transactions::prepare_transaction(
        tid,
        resource,
        balance_change,
        prepare_balance,
    );
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
    crate::atomic_transactions::abort_transaction(tid, resource);
    print_state();
}

#[update]
/// Commit changes according to previously prepared balance change and resource.
///
/// If this fails, there is likely a bug in the protocol.
fn commit_transaction(tid: TransactionId, resource: TokenName, balance_change: i64) {
    ic_cdk::println!("Committing transaction: {} for token {:?}", tid, resource);
    crate::atomic_transactions::commit_transaction(tid, resource, balance_change, commit_balance);
    print_state();
}

#[update]
/// Prepare initial balances of this ledger
///
/// This initializes the ledger with the given token names and balances.
/// No initialization of the atomic transactions state is necessary.
fn init(token_names: Vec<TokenName>, token_balances: Vec<TokenBalance>) {
    with_balances_mut(|balances| {
        for (name, balance) in token_names.iter().zip(token_balances) {
            balances.insert(name.clone(), balance);
            ic_cdk::println!("Ledger: Inital token {:?} with balance {}", name, balance);
        }
    });
}

fn print_state() {
    ic_cdk::println!("Current state of ledger:");
    with_balances_mut(|balances| {
        for (token, balance) in balances.iter() {
            ic_cdk::println!("Token balance: {:?} {:?}", token, balance);
        }
    });

    crate::atomic_transactions::print_state();
}
