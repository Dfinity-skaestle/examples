use std::collections::BTreeMap;

use candid::{CandidType, Principal};

pub(crate) type CanisterId = Principal;
pub(crate) type TransactionId = usize;

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
}

pub(crate) struct TransactionState {
    pub(crate) total_number_of_children: u64, // Total number of canisters participating in 2PC
    pub(crate) transaction_status: TransactionStatus,
    pub(crate) pending_prepare_calls: Vec<Call>,
    pub(crate) pending_abort_calls: Vec<Call>,
    pub(crate) pending_commit_calls: Vec<Call>,
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
        }
    }

    pub(crate) fn prepare_received(&mut self, success: bool, canister_id: CanisterId) {
        assert_eq!(self.transaction_status, TransactionStatus::Preparing);

        let call = self
            .pending_prepare_calls
            .iter()
            .find(|call| call.target == canister_id)
            .unwrap();

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

        if num_succ_calls as u64 == self.total_number_of_children {
            self.transaction_status = TransactionStatus::Committing;
        }
    }

    pub(crate) fn abort_received(&mut self, success: bool, canister_id: CanisterId) {
        assert_eq!(self.transaction_status, TransactionStatus::Aborting);

        let call = self
            .pending_abort_calls
            .iter()
            .find(|call| call.target == canister_id)
            .unwrap();

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
            .iter()
            .find(|call| call.target == canister_id)
            .unwrap();

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
            .iter()
            .find(|call| call.target == canister_id)
            .unwrap();

        call.num_tries += 1;
    }

    pub(crate) fn register_abort_call(&mut self, canister_id: CanisterId) {
        let call = self
            .pending_abort_calls
            .iter()
            .find(|call| call.target == canister_id)
            .unwrap();

        call.num_tries += 1;
    }

    pub(crate) fn register_commit_call(&mut self, canister_id: CanisterId) {
        let call = self
            .pending_commit_calls
            .iter()
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

#[derive(Clone)]
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

pub(crate) fn get_transaction_state(
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
