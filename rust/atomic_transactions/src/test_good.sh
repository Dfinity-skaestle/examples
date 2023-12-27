#!/bin/bash

set -euox pipefail

function error() {
    echo "$1"
    exit 1
}

dfx canister call dex init '()'
dfx canister call dex disable_timer '(true)'

dfx canister call dex swap_tokens '("ICP", "USD", -1337, 47)'
dfx canister call dex transaction_loop '(0)'
dfx canister call dex transaction_loop '(0)' | grep "Committed" || error "Transaction 0 was not committed"
