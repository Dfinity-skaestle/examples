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
    // Prepare
    pub(crate) pending_prepare_calls: Vec<Call>,
    pub(crate) num_prepare_okay: u64,
    pub(crate) num_prepare_fail: u64,
    // Abort
    pub(crate) pending_abort_calls: Vec<Call>,
    pub(crate) num_abort_okay: u64,
    pub(crate) num_abort_fail: u64,
    // Commit
    pub(crate) pending_commit_calls: Vec<Call>,
    pub(crate) num_commit_okay: u64,
    pub(crate) num_commit_fail: u64,
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
            .map(|(canister_id, payload)| Call {
                target: canister_id.clone(),
                method: method_prepare.to_string(),
                payload: payload.to_vec(),
            })
            .collect::<Vec<Call>>();

        let abort_calls = canisters
            .iter()
            .zip(payload.iter())
            .map(|(canister_id, payload)| Call {
                target: canister_id.clone(),
                method: method_abort.to_string(),
                payload: payload.to_vec(),
            })
            .collect::<Vec<Call>>();

        let commit_calls = canisters
            .iter()
            .zip(payload.iter())
            .map(|(canister_id, payload)| Call {
                target: canister_id.clone(),
                method: method_commit.to_string(),
                payload: payload.to_vec(),
            })
            .collect::<Vec<Call>>();

        TransactionState {
            total_number_of_children: canisters.len() as u64,
            transaction_status: TransactionStatus::Preparing,
            // Prepare
            pending_prepare_calls: prepare_calls,
            // XXX - These should really not be counts, but the state for each of the canisters involved
            num_prepare_okay: 0,
            num_prepare_fail: 0,
            // Abort
            pending_abort_calls: abort_calls,
            num_abort_okay: 0,
            num_abort_fail: 0,
            // Commit
            pending_commit_calls: commit_calls,
            num_commit_okay: 0,
            num_commit_fail: 0,
        }
    }

    pub(crate) fn prepare_received(&mut self, success: bool) {
        assert_eq!(self.transaction_status, TransactionStatus::Preparing);

        if success {
            self.num_prepare_okay += 1;
        } else {
            self.num_prepare_fail += 1;
        }

        if self.num_prepare_okay == self.total_number_of_children {
            // Okay received from all children for prepare
            self.transaction_status = TransactionStatus::Committing;
        } else if self.num_prepare_fail + self.num_prepare_okay == self.total_number_of_children {
            // Received a response from each child, but not all sent "okay"
            self.transaction_status = TransactionStatus::Aborting;
        }
    }

    pub(crate) fn abort_received(&mut self, success: bool) {
        assert_eq!(self.transaction_status, TransactionStatus::Aborting);

        if success {
            self.num_abort_okay += 1;
        } else {
            self.num_abort_fail += 1;
        }

        if self.num_abort_okay == self.total_number_of_children {
            // Okay received from all children for abort
            self.transaction_status = TransactionStatus::Aborted;
        } else if self.num_abort_fail + self.num_abort_okay == self.total_number_of_children {
            // Retry
            self.num_abort_fail = 0;
            self.num_abort_okay = 0;
            // No state change
        }
    }

    pub(crate) fn commit_received(&mut self, success: bool) {
        assert_eq!(self.transaction_status, TransactionStatus::Committing);

        if success {
            self.num_commit_okay += 1;
        } else {
            self.num_commit_fail += 1;
        }

        if self.num_commit_okay == self.total_number_of_children {
            // Okay received from all children for commit
            self.transaction_status = TransactionStatus::Committed;
        } else if self.num_commit_fail + self.num_commit_okay == self.total_number_of_children {
            // Retry
            self.num_commit_fail = 0;
            self.num_commit_okay = 0;
            // No state change
        }
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
