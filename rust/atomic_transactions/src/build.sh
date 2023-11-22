#!/bin/bash

set -euo pipefail

BASE="../target/wasm32-unknown-unknown/release"

echo "building data partition canister"

cargo build --target wasm32-unknown-unknown --release -p ledger_token_1 --locked
ic-cdk-optimizer ${BASE}/ledger_token_1.wasm --output ${BASE}/ledger_token_1.wasm

cargo build --target wasm32-unknown-unknown --release -p ledger_token_2 --locked
ic-cdk-optimizer ${BASE}/ledger_token_2.wasm --output ${BASE}/ledger_token_2.wasm

(
    echo "compressing data partition canister"
    cd ${BASE}
    gzip -c ledger_token_1.wasm > ledger_token_1.wasm.gz
    gzip -c ledger_token_2.wasm > ledger_token_2.wasm.gz
)

echo "building kv frontend canister"
cargo build --target wasm32-unknown-unknown --release -p kv_frontend --locked
ic-cdk-optimizer ${BASE}/kv_frontend.wasm --output ./kv_frontend.wasm