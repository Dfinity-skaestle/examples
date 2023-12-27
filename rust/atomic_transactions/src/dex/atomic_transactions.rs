use std::{cell::RefCell, collections::BTreeMap, time::Duration};

use ansi_term::Style;
use candid::{CandidType, Decode, Principal};
use ic_cdk::api::call::call_raw;
use ic_cdk_macros::{query, update};

pub(crate) type CanisterId = Principal;
pub(crate) type TransactionId = usize;

// The time in nanoseconds after which a transaction is aborted if the prepare phase has not finished.
// This prevents malicious canisters from locking a transaction forever without
// freeing resources.
//
// Malicious canisters can still consumes resources forever if the inifinitely delay the commit call.
// That's because commits need to be retried forever. Not though, that other canisters involved in the 2PC can
// unlock and continue their computation right after they have executed the commit, allowing them to
// unlock all of their resources.
//
// This cannot be avoided easily, as the 2PC protocols dictacts that the commit has to happen if the
// prepare phase was successful.
//
// One solution might be to roll back state, but that is complicated and it is hard to guarantee that
// the roll back doesn't fail either.
//
// Generally, calling into a canister assumes some sort of trust for that canister.
const ABORT_PREPARE_AFTER_NS: u64 = 10 * 1000 * 1000 * 1000;

// The minimum time in nanoseconds between consecutive actions on the same transactions.
// This is to prevent a transaction from being executed too often.
const RATE_LIMIT_TIMEOUT_NS: u64 = 5 * 1000 * 1000 * 1000;

#[derive(Default, Clone, Copy)]
struct Configuration {
    disable_timer: bool,
}

#[derive(CandidType, Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TransactionStatus {
    // Active states
    Preparing,
    Aborting,
    Committing,
    // Final states
    Committed,
    Aborted,
}

thread_local! {
    // A list of canister IDs for data partitions
    static TRANSACTION_STATE: RefCell<TransactionList> = RefCell::new(TransactionList::default());
    static CONFIGURATION: RefCell<Configuration> = RefCell::new(Configuration::default());
}

/// A helper method to mutate the state.
pub(crate) fn with_state_mut<R>(f: impl FnOnce(&mut TransactionList) -> R) -> R {
    TRANSACTION_STATE.with(|cell| f(&mut cell.borrow_mut()))
}

/// A helper method to access the state.
pub(crate) fn with_state<R>(f: impl FnOnce(&TransactionList) -> R) -> R {
    TRANSACTION_STATE.with(|cell| f(&cell.borrow()))
}

/// A helper method to mutate the state.
pub(crate) fn with_transaction_mut<R>(
    tid: TransactionId,
    f: impl FnOnce(TransactionId, &mut TransactionState) -> R,
) -> R {
    TRANSACTION_STATE.with(|cell| f(tid, cell.borrow_mut().transactions.get_mut(&tid).unwrap()))
}

/// A helper method to access the state.
pub(crate) fn with_transaction<R>(
    tid: TransactionId,
    f: impl FnOnce(TransactionId, &TransactionState) -> R,
) -> R {
    TRANSACTION_STATE.with(|cell| f(tid, cell.borrow().transactions.get(&tid).unwrap()))
}

fn get_configuration() -> Configuration {
    CONFIGURATION.with(|configuration| configuration.borrow().clone())
}

pub(crate) struct TransactionList {
    next_transaction_number: TransactionId,
    pub(crate) transactions: BTreeMap<TransactionId, TransactionState>,
}

impl Default for TransactionList {
    fn default() -> Self {
        Self {
            next_transaction_number: 0,
            transactions: BTreeMap::new(),
        }
    }
}

impl TransactionList {
    pub(crate) fn get_next_transaction_number(&mut self) -> TransactionId {
        let transaction_number = self.next_transaction_number;
        self.next_transaction_number += 1;
        transaction_number
    }

    pub(crate) fn add_transaction(
        &mut self,
        transaction_state: TransactionState,
        transaction_number: usize,
    ) {
        self.transactions
            .insert(transaction_number, transaction_state);
    }
}

#[derive(Debug)]
pub(crate) struct TransactionState {
    pub(crate) total_number_of_children: u64, // Total number of canisters participating in 2PC
    pub(crate) transaction_status: TransactionStatus,
    pub(crate) pending_prepare_calls: Vec<Call>,
    pub(crate) pending_abort_calls: Vec<Call>,
    pub(crate) pending_commit_calls: Vec<Call>,
    // The time this transaction was last retried.
    pub(crate) last_action_time: u64,
    // The time at which the prepare phase has started.
    pub(crate) prepare_start_time: u64,
}

impl TransactionState {
    pub(crate) fn new(
        canisters: &[CanisterId],
        method_prepare: &str,
        method_abort: &str,
        method_commit: &str,
        payload: &[&[u8]],
    ) -> Self {
        let prepare_calls = canisters
            .iter()
            .zip(payload.iter())
            .map(|(canister_id, payload)| {
                Call::new(
                    canister_id.clone(),
                    method_prepare.to_string(),
                    payload.to_vec(),
                )
            })
            .collect::<Vec<Call>>();

        let abort_calls = canisters
            .iter()
            .zip(payload.iter())
            .map(|(canister_id, payload)| {
                Call::new(
                    canister_id.clone(),
                    method_abort.to_string(),
                    payload.to_vec(),
                )
            })
            .collect::<Vec<Call>>();

        let commit_calls = canisters
            .iter()
            .zip(payload.iter())
            .map(|(canister_id, payload)| {
                Call::new(
                    canister_id.clone(),
                    method_commit.to_string(),
                    payload.to_vec(),
                )
            })
            .collect::<Vec<Call>>();

        TransactionState {
            total_number_of_children: canisters.len() as u64,
            transaction_status: TransactionStatus::Preparing,
            pending_prepare_calls: prepare_calls,
            pending_abort_calls: abort_calls,
            pending_commit_calls: commit_calls,
            last_action_time: 0,
            prepare_start_time: ic_cdk::api::time(),
        }
    }

    pub(crate) fn prepare_received(&mut self, success: bool, canister_id: CanisterId) {
        // We are either in Preparing state or the transaction has already been aborted and
        // we still receive from stranglers.
        assert!(
            self.transaction_status == TransactionStatus::Preparing
                || self.transaction_status == TransactionStatus::Aborting
        );

        let call = self
            .pending_prepare_calls
            .iter_mut()
            .find(|call| call.target == canister_id)
            .unwrap();

        ic_cdk::println!(
            "Received prepare response from {} with success {}",
            canister_id,
            success
        );

        if success {
            call.num_success += 1;
        } else {
            call.num_fail += 1;
        }

        let num_succ_calls = self
            .pending_prepare_calls
            .iter()
            .filter(|call| call.num_success > 0)
            .count();

        // All peers approved the prepare statement
        if num_succ_calls as u64 == self.total_number_of_children {
            self.transaction_status = TransactionStatus::Committing;
        }

        let num_fail_calls = self
            .pending_prepare_calls
            .iter()
            .filter(|call| call.num_fail > 0)
            .count();

        // At least one peer rejected the prepare statement
        if num_fail_calls as u64 > 0 {
            self.transaction_status = TransactionStatus::Aborting;
        }
    }

    pub(crate) fn abort_received(&mut self, success: bool, canister_id: CanisterId) {
        assert_eq!(self.transaction_status, TransactionStatus::Aborting);

        let call = self
            .pending_abort_calls
            .iter_mut()
            .find(|call| call.target == canister_id)
            .unwrap();

        ic_cdk::println!(
            "Received abort response from {} with success {}",
            canister_id,
            success
        );

        if success {
            call.num_success += 1;
        } else {
            call.num_fail += 1;
        }

        let num_succ_calls = self
            .pending_abort_calls
            .iter()
            .filter(|call| call.num_success > 0)
            .count();

        if num_succ_calls as u64 == self.total_number_of_children {
            self.transaction_status = TransactionStatus::Aborted;
        }
    }

    pub(crate) fn commit_received(&mut self, success: bool, canister_id: CanisterId) {
        assert_eq!(self.transaction_status, TransactionStatus::Committing);

        let call = self
            .pending_commit_calls
            .iter_mut()
            .find(|call| call.target == canister_id)
            .unwrap();

        ic_cdk::println!(
            "Received commit response from {} with success {}",
            canister_id,
            success
        );

        if success {
            call.num_success += 1;
        } else {
            call.num_fail += 1;
        }

        // Total number of successful Call instances with successful calls
        let num_succ_calls = self
            .pending_commit_calls
            .iter()
            .filter(|call| call.num_success > 0)
            .count();

        if num_succ_calls as u64 == self.total_number_of_children {
            self.transaction_status = TransactionStatus::Committed;
        }
    }

    pub(crate) fn register_prepare_call(&mut self, canister_id: CanisterId) {
        let call = self
            .pending_prepare_calls
            .iter_mut()
            .find(|call| call.target == canister_id)
            .unwrap();

        call.num_tries += 1;
    }

    pub(crate) fn register_abort_call(&mut self, canister_id: CanisterId) {
        let call = self
            .pending_abort_calls
            .iter_mut()
            .find(|call| call.target == canister_id)
            .unwrap();

        call.num_tries += 1;
    }

    pub(crate) fn register_commit_call(&mut self, canister_id: CanisterId) {
        let call = self
            .pending_commit_calls
            .iter_mut()
            .find(|call| call.target == canister_id)
            .unwrap();

        call.num_tries += 1;
    }
}

#[derive(CandidType, Debug)]
pub(crate) struct TransactionResult {
    pub(crate) transaction_number: TransactionId,
    pub(crate) state: TransactionStatus,
}

#[derive(Clone, Debug)]
pub(crate) struct Call {
    pub(crate) target: CanisterId,
    pub(crate) method: String,
    pub(crate) payload: Vec<u8>,
    // Track number of calls and number of failures
    pub(crate) num_tries: u64,
    pub(crate) num_success: u64,
    pub(crate) num_fail: u64,
}

impl Call {
    fn new(target: CanisterId, method: String, payload: Vec<u8>) -> Self {
        Self {
            target,
            method,
            payload,
            num_tries: 0,
            num_success: 0,
            num_fail: 0,
        }
    }
}

fn _get_transaction_result(
    tid: TransactionId,
    transaction_state: &TransactionState,
) -> TransactionResult {
    TransactionResult {
        transaction_number: tid,
        state: transaction_state.transaction_status,
    }
}

pub(crate) fn get_transaction_status(
    _tid: TransactionId,
    transaction_state: &TransactionState,
) -> TransactionStatus {
    transaction_state.transaction_status
}

#[query]
/// Get the current state of a transaction.
pub fn get_transaction_state(tid: TransactionId) -> TransactionResult {
    with_transaction(tid, _get_transaction_result)
}

pub fn get_next_transaction_number() -> TransactionId {
    TRANSACTION_STATE.with(|cell| cell.borrow_mut().get_next_transaction_number())
}

pub fn add_transaction(
    transaction_state: TransactionState,
    transaction_number: usize,
) -> TransactionResult {
    with_state_mut(|state| {
        state.add_transaction(transaction_state, transaction_number);
    });
    get_transaction_state(transaction_number)
}

fn get_active_transactions() -> Vec<TransactionId> {
    with_state(|state| {
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

#[update]
pub fn disable_timer(disable: bool) {
    CONFIGURATION.with(|configuration| {
        configuration.borrow_mut().disable_timer = disable;
    });
}

#[update]
/// Resume executing a transaction.
///
/// Calling this function might change the state of the transaction.
/// This can either be triggered peridocially by the user or by a timer.
///
/// Returns the state of the transaction.
async fn timer_loop() {
    // The timer has to be set first!
    // Otherwise, if a call triggered from the timer never returns, no more new timers will be scheduled.
    // XXX Optimization: Schedule timer not every 1 second, but based on the state of active transactions.
    ic_cdk_timers::set_timer(Duration::from_secs(1), || ic_cdk::spawn(timer_loop()));

    if !get_configuration().disable_timer {
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
    }
}

#[update]
pub async fn transaction_loop(tid: TransactionId) -> TransactionResult {
    let initial_transaction_status = with_transaction(tid, get_transaction_status);
    ic_cdk::println!(
        "Executing transaction {} with status {:?}",
        tid,
        initial_transaction_status
    );

    if ic_cdk::api::time()
        <= with_transaction(tid, |_, s| s.last_action_time) + RATE_LIMIT_TIMEOUT_NS
    {
        ic_cdk::println!("Rate limiting transaction {}", tid);
        return get_transaction_state(tid);
    }

    match initial_transaction_status {
        TransactionStatus::Preparing => {
            // Check if the prepare phase has timed out
            let timeout =
                with_transaction(tid, |_, s| s.prepare_start_time + ABORT_PREPARE_AFTER_NS);
            if ic_cdk::api::time() > timeout {
                ic_cdk::println!(
                    "Aborting transaction {} because prepare phase timed out",
                    tid
                );
                with_transaction_mut(tid, |_, s| {
                    s.transaction_status = TransactionStatus::Aborting
                });
            } else {
                let pending_prepare_calls =
                    with_transaction(tid, |_, f| f.pending_prepare_calls.clone());

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

                    with_transaction_mut(tid, |_, s| s.last_action_time = ic_cdk::api::time());

                    with_transaction_mut(tid, |_, s| s.register_prepare_call(call.target.clone()));
                    let call_raw_result =
                        call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                    with_transaction_mut(tid, |_, s| {
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
                                ic_cdk::println!(
                                    "Received prepare response: {}",
                                    successful_prepare
                                );
                                successful_prepare
                            }
                            Err(_) => false,
                        };
                        s.prepare_received(succ, call.target)
                    });
                }
            }
        }
        TransactionStatus::Aborting => {
            let pending_abort_calls = with_transaction(tid, |_, f| f.pending_abort_calls.clone());

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

                with_transaction_mut(tid, |_, s| s.last_action_time = ic_cdk::api::time());

                with_transaction_mut(tid, |_, s| s.register_abort_call(call.target.clone()));
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                with_transaction_mut(tid, |_, s| {
                    s.abort_received(call_raw_result.is_ok(), call.target)
                });
            }
        }
        TransactionStatus::Committing => {
            let pending_commit_calls =
                with_transaction_mut(tid, |_, f| f.pending_commit_calls.clone());

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

                with_transaction_mut(tid, |_, s| s.last_action_time = ic_cdk::api::time());

                with_transaction_mut(tid, |_, s| s.register_commit_call(call.target.clone()));
                let call_raw_result =
                    call_raw(call.target, &call.method, call.payload.clone(), 0).await;

                with_transaction_mut(tid, |_, s| {
                    s.commit_received(call_raw_result.is_ok(), call.target)
                });
            }
        }
        // We are already in a final state, no need to do anything
        TransactionStatus::Committed => {}
        TransactionStatus::Aborted => {}
    }

    let new_transaction_status = with_transaction(tid, |_, state| {
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
        with_transaction_mut(tid, |_, s| s.last_action_time = 0);
    }

    get_transaction_state(tid)
}

pub fn init() {
    // Reset transactions
    with_state_mut(|state| {
        *state = TransactionList::default();
    });

    ic_cdk_timers::set_timer(Duration::from_secs(1), || {
        ic_cdk::spawn(timer_loop());
    });
}
