## Build & Test
You can build and test the code normally with cargo.

```bash
cargo test
```

To get a deployable wasm artifact, you also need the wasm toolchain and the soroban cli. See here: https://soroban.stellar.org/docs/getting-started/setup

```bash
soroban contract build --package dex-market
```

## Deploy to a local validator
Run this script to start a local validator in docker and deploy dex_market to it:

```bash
./validator.sh
```

To stop the validator:
```bash
./validator.sh stop
```

Dependencies:
- docker
- soroban cli

More info:
- https://github.com/stellar/quickstart
- https://soroban.stellar.org/docs/reference/rpc
- https://soroban.stellar.org/docs/getting-started/deploy-to-testnet
