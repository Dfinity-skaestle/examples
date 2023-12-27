#!/bin/bash

set -euox pipefail

function error() {
    echo "$1"
    exit 1
}

dfx canister call dex init '()'
dfx canister call dex disable_timer '(true)'

# Make sure there are not outstanding timer invocations
sleep 2

dfx canister call dex swap_tokens '("ICP", "USD", -1337, 47)'
dfx canister call dex transaction_loop '(0)'

dfx canister call dex swap_tokens '("ICP", "EUR", -2000, 64)'
dfx canister call dex transaction_loop '(1)'
dfx canister call dex transaction_loop '(1)' | grep "Aborted" || error "Transaction 1 was not aborted"

dfx canister call dex transaction_loop '(0)' | grep "Committed" || error "Transaction 0 was not committed"
