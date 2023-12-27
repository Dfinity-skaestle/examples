#!/bin/bash

set -euox pipefail

function error() {
    echo "$1"
    exit 1
}

dfx canister call dex init '()'
dfx canister call dex disable_timer '(false)'
sleep 1

dfx canister call dex swap_tokens '("ICP", "USD", -1337, 47)'

echo "Sleeping - the timer should finish the transaction."
sleep 3
dfx canister call dex get_transaction_state '(0)' | grep "Committed" || error "Transaction 0 was not committed"
