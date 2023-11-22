#!/bin/bash

set -euox pipefail

BASE="../target/wasm32-unknown-unknown/release"

echo "building data partition canister"

cargo build --target wasm32-unknown-unknown --release -p ledger_token_1 --locked
ic-wasm ${BASE}/ledger_token_1.wasm optimize O2

cargo build --target wasm32-unknown-unknown --release -p ledger_token_2 --locked
ic-wasm ${BASE}/ledger_token_2.wasm optimize  O2

(
    echo "compressing data partition canister"
    cd ${BASE}
    gzip -c ledger_token_1.wasm > ledger_token_1.wasm.gz
    gzip -c ledger_token_2.wasm > ledger_token_2.wasm.gz
)

echo "building kv frontend canister"
cargo build --target wasm32-unknown-unknown --release -p dex --locked
ic-wasm ${BASE}/dex.wasm optimize  O2; cp ${BASE}/dex.wasm ./dex.wasm