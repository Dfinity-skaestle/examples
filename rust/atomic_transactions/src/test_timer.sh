#!/bin/bash

set -euox pipefail

function error() {
    echo "$1"
    exit 1
}

dfx canister call dex init '()'

dfx canister call dex swap_token1_to_token2 '()'

echo "Sleeping for 10 seconds - the timer should finish the transaction."
sleep 3
dfx canister call dex get_transaction_state '(0: nat64)' | grep "Committed" || error "Transaction 0 was not committed"
