#!/usr/bin/env bash
set -euxo pipefail

# configure soroban cli to deploy to localnet and an identity to deploy to localnet
config-cli() {
    soroban config network add --global local \
        --rpc-url http://localhost:8000/soroban/rpc \
        --network-passphrase "Standalone Network ; February 2017"

    soroban config identity generate --global local-deployer
}

# start a stellar validator in docker in the background
start() {
    docker run --rm -d \
        -p "8000:8000" \
        --name stellar \
        stellar/quickstart:testing \
        --local \
        --enable-soroban-rpc
}

# stop the stellar validator
stop() {
    docker stop stellar
}

# wait for validator to be live with at least as many blocks
await-startup() { local blocks=${1:-0};
    set +x
    echo waiting for local validator to start...
    while ! poll $blocks; do sleep 1; done
    echo validator is live.
    set -x
}

# return if validator is live with at least as many blocks
poll() { local blocks=$1;
    [[ $(curl -sf "http://localhost:8000" | jq .history_latest_ledger) -gt $blocks ]]
}

# deploy a contract to soroban
deploy() { local contract=$1;
    soroban contract deploy \
            --wasm "target/wasm32-unknown-unknown/release/$contract.wasm" \
            --source local-deployer \
            --network local
}

# airdrop local-deployer once the validator is ready
await-startup-and-airdrop-deployer() {
    await-startup 0
    for blocks in 10 20 30 quit; do
        if ! soroban config identity fund local-deployer --network local; then
            if [[ $blocks == quit ]]; then
                return 1
            fi
            await-startup $blocks
        else
            break
        fi
    done
}

# start a fresh validator and deploy our contract
full() {
    stop && sleep 0.1 || true
    config-cli
    start
    await-startup-and-airdrop-deployer
    deploy dex_market
}


if [[ "$@" == '' ]]; then
    full
else
    $@
fi
