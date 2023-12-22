#!/bin/bash

set -euox pipefail

function error() {
    echo "$1"
    exit 1
}

dfx build dex 
dfx canister install dex --mode=reinstall -y