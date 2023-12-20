#!/bin/bash

set -euox pipefail

dfx build dex 
dfx canister install dex --mode=reinstall -y
dfx canister call dex init '()'
dfx canister call dex swap_token1_to_token2 '()'
dfx canister call dex transaction_loop '(0: nat64)'
dfx canister call dex transaction_loop '(0: nat64)'