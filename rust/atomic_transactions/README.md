# Installing

# Installing

We first need to build the data partition backend canister.

```
cd rust/atomic_transactions/src
dfx start
dfx canister create dex
dfx build dex
gzip dex.wasm
dfx canister install dex
dfx canister call dex init '()'
dfx canister call dex swap_token1_to_token2 '()'
dfx canister call dex transaction_loop '(0)'
```

