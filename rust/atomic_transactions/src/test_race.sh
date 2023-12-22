#!/bin/bash

set -euox pipefail

function error() {
    echo "$1"
    exit 1
}

dfx canister call dex init '()'

dfx canister call dex swap_token1_to_token2 '()'
dfx canister call dex transaction_loop '(0: nat64)'

dfx canister call dex swap_tokens '("ICP", "EUR", -2000: int64, 64: int64)'
dfx canister call dex transaction_loop '(1: nat64)'
dfx canister call dex transaction_loop '(1: nat64)' | grep "Aborted" || error "Transaction 1 was not aborted"

dfx canister call dex transaction_loop '(0: nat64)' | grep "Committed" || error "Transaction 0 was not committed"
