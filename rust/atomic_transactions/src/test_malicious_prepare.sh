#!/bin/bash

set -euox pipefail

function error() {
    echo "$1"
    exit 1
}

dfx canister call dex init '()'
# We need timers in this tests, since the timer will abort the transaction
dfx canister call dex disable_timer '(false)'
dfx canister call dex set_configuration 'record { stop_on_prepare = true; infinite_prepare = false; }'

dfx canister call dex swap_tokens '("ICP", "USD", -1337, 47)'
dfx canister call dex transaction_loop '(0)'

sleep 20

dfx canister call dex transaction_loop '(0)' | grep "Aborted" || error "Transaction 0 was not aborted"
