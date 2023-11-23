use ic_cdk_macros::update;
use ic_stable_structures::{BTreeMap, BoundedStorable, DefaultMemoryImpl, Storable};
use std::cell::RefCell;

type TokenName = u32;
type TokenBalance = u64;
type TransactionId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransactionState {
    Prepared,
    Aborted,
    Comitted,
}

impl Storable for TransactionState {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        let r: u8 = match self {
            TransactionState::Prepared => 1,
            TransactionState::Aborted => 2,
            TransactionState::Comitted => 3,
        };
        std::borrow::Cow::Owned(vec![r])
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        let ptr = bytes.as_ptr();
        let v = unsafe { *ptr };

        match v {
            1 => TransactionState::Prepared,
            2 => TransactionState::Aborted,
            3 => TransactionState::Comitted,
            _ => panic!("Unsupported transaction state"),
        }
    }
}

impl BoundedStorable for TransactionState {
    const MAX_SIZE: u32 = 1;
    const IS_FIXED_SIZE: bool = true;
}

thread_local! {
    // Balances of tokens stored in this ledger
    static BALANCES: RefCell<BTreeMap<TokenName, TokenBalance, DefaultMemoryImpl>> = RefCell::new(
        BTreeMap::init(DefaultMemoryImpl::default()));

    // Balances of tokens stored in this ledger
    static PC_STATE: RefCell<BTreeMap<TokenName, TransactionState, DefaultMemoryImpl>> = RefCell::new(
        BTreeMap::init(DefaultMemoryImpl::default()));
}

#[update]
/// Prepare initial balances of this ledger
fn init(initial_token_balances: Vec<(TokenName, TokenBalance)>) {
    BALANCES.with_borrow_mut(|balances| {
        for (name, balance) in initial_token_balances {
            balances.insert(name, balance);
        }
    });
}

#[update]
/// Prepare atomic transactions by means of Two-Phase Commit
///
/// This code ensures that resource exists and that the change in balance does not create overflows.
/// It also ensures that the given resource has not already been prepared by another transaction.
/// If this is okay, response "yes", otherwise "no".
fn prepare_transaction(tid: TransactionId, resource: TokenName, balance_change: i64) -> bool {
    ic_cdk::println!("Preparing transaction: {}", tid);

    PC_STATE.with_borrow_mut(|pc_state| {
        BALANCES.with_borrow_mut(|balances| {
            match pc_state.get(&resource) {
                Some(TransactionState::Prepared) => {
                    // Resource already in prepare state, reject further prepare statements.
                    false
                }
                Some(TransactionState::Aborted) | Some(TransactionState::Comitted) | None => {
                    match balances.get(&resource) {
                        Some(resource_balance) => {
                            // Check if given balance exists and if overflow would happen for the given change in balance
                            match resource_balance.checked_add_signed(balance_change) {
                                Some(_) => {
                                    // Resource not locked in 2PC
                                    pc_state.insert(resource, TransactionState::Prepared);
                                    true
                                }
                                None => false,
                            }
                        }
                        None => false,
                    }
                }
            }
        })
    })
}

#[update]
/// Abort previously prepared transaction.
///
/// Resets the state of the given resource to "aborted". This will free up resources that have
/// previously been locked by responding "yes" to previous "prepare" requests.
fn abort_transaction(tid: TransactionId, resource: TokenName) {
    ic_cdk::println!("Aborting transaction: {}", tid);
    PC_STATE.with_borrow_mut(|pc_state| {
        pc_state.insert(resource, TransactionState::Aborted);
    })
}

#[update]
/// Commit changes according to previously prepared balance change and resource.
///
/// If this fails, there is likely a bug in the protocol.
fn commit_transaction(tid: TransactionId, resource: TokenName, balance_change: i64) {
    ic_cdk::println!("Committing transaction: {}", tid);
    PC_STATE.with_borrow_mut(|pc_state| {
        assert_eq!(pc_state.get(&resource), Some(TransactionState::Prepared));
        BALANCES.with_borrow_mut(|balances| {
            balances.insert(
                resource.clone(),
                balances
                    .get(&resource)
                    .expect("Token does not have a registered balance - prepare should have failed")
                    .checked_add_signed(balance_change)
                    .expect("Token balance overflow - prepare should have failed"),
            );
            pc_state.insert(resource, TransactionState::Comitted);
        });
    })
}
