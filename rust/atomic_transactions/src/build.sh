#!/bin/bash

# This is a build script for dfx.
# Do not run this manually.

set -euox pipefail

BASE="../target/wasm32-unknown-unknown/release"

echo "building ledger canister"

cargo build --target wasm32-unknown-unknown --release -p ledger_token_1 --locked
ic-wasm ${BASE}/ledger_token_1.wasm optimize O2

(
    echo "compressing ledger canister"
    cd ${BASE}
    gzip -c ledger_token_1.wasm > ledger_token_1.wasm.gz
)

echo "building dex canister"
cargo build --target wasm32-unknown-unknown --release -p dex --locked
ic-wasm ${BASE}/dex.wasm optimize  O2; cp ${BASE}/dex.wasm ./dex.wasm
gzip -c dex.wasm > dex.wasm.gz