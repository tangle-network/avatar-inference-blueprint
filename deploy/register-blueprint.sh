#!/usr/bin/env bash
# Register the avatar-inference blueprint on Tangle.
#
# Three-stage flow (Pattern A — cargo-tangle CLI):
#   1. forge create AvatarBSM implementation contract.
#   2. forge create ERC1967Proxy in front of the impl, with initialize(paymentToken)
#      encoded as the proxy constructor's `_data` so the proxy boots initialized
#      in a single transaction.
#   3. cargo tangle blueprint deploy tangle — registers the blueprint via the
#      definition file at `deploy/definition.json` with the freshly-deployed BSM
#      proxy address patched in as the manager.
#
# AvatarBSM is a UUPS-upgradeable contract (extends Initializable +
# UUPSUpgradeable + BlueprintServiceManagerBase), so its `manager` address must
# be the proxy, not the implementation. The constructor is empty (no args); the
# payment token is wired via initialize(address) on the proxy.
#
# Prerequisites:
#   - forge (Foundry) installed
#   - cargo-tangle CLI installed (`cargo install cargo-tangle`)
#   - jq installed
#   - Deployer wallet funded on the target network
#   - Keystore with the deployer key at ./keystore (or set KEYSTORE_PATH)
#
# Usage (Base Sepolia, against the already-deployed Tangle protocol):
#
#   export PRIVATE_KEY=0x...
#   export RPC_URL=https://sepolia.base.org
#   export WS_URL=wss://base-sepolia-rpc.publicnode.com
#   export TANGLE_CORE=0xC9b0716a187072be0f38A5D972392C6479b9Cfe3
#   export PAYMENT_TOKEN=0x036CbD53842c5426634e7929541eC2318f3dCF7e  # USDC sepolia
#   export KEYSTORE_PATH=./keystore
#   ./deploy/register-blueprint.sh
#
# Optional overrides:
#   BSM_PROXY_ADDRESS  — skip the forge-create steps and reuse an already-deployed
#                        BSM proxy. definition.json gets patched with this address
#                        and only cargo-tangle runs.

set -euo pipefail

: "${RPC_URL:?Set RPC_URL}"
: "${PRIVATE_KEY:?Set PRIVATE_KEY}"
: "${WS_URL:?Set WS_URL (ws://… or wss://…)}"

# Base Sepolia defaults (override via env when targeting another network).
TANGLE_CORE="${TANGLE_CORE:-0xC9b0716a187072be0f38A5D972392C6479b9Cfe3}"
PAYMENT_TOKEN="${PAYMENT_TOKEN:-0x036CbD53842c5426634e7929541eC2318f3dCF7e}"
KEYSTORE_PATH="${KEYSTORE_PATH:-./keystore}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONTRACTS_DIR="$REPO_ROOT/contracts"
DEFINITION_FILE="$REPO_ROOT/deploy/definition.json"

echo "=== Avatar-Inference Blueprint Registration ==="
echo "Network:       $(cast chain-id --rpc-url "$RPC_URL")"
echo "Deployer:      $(cast wallet address --private-key "$PRIVATE_KEY")"
echo "Tangle Core:   $TANGLE_CORE"
echo "Payment Token: $PAYMENT_TOKEN"
echo "Definition:    $DEFINITION_FILE"
echo ""

# ── Stage 1+2 — Deploy AvatarBSM (impl + UUPS proxy + initialize). ────────────
# Skipped if BSM_PROXY_ADDRESS is already set in the environment.
if [ -z "${BSM_PROXY_ADDRESS:-}" ]; then
    echo "Stage 1: deploying AvatarBSM implementation …"
    BSM_IMPL_ADDRESS=$(forge create \
        --rpc-url "$RPC_URL" \
        --private-key "$PRIVATE_KEY" \
        --broadcast \
        --root "$CONTRACTS_DIR" \
        "$CONTRACTS_DIR/src/AvatarBSM.sol:AvatarBSM" \
        2>&1 | grep -oE 'Deployed to: 0x[a-fA-F0-9]{40}' | tail -1 | awk '{print $3}')
    echo "AvatarBSM impl deployed at: $BSM_IMPL_ADDRESS"

    echo ""
    echo "Stage 2: deploying ERC1967Proxy (initialize(paymentToken=$PAYMENT_TOKEN)) …"
    # initialize(address) selector = 0xc4d66de8
    INIT_DATA=$(cast calldata "initialize(address)" "$PAYMENT_TOKEN")
    # NOTE: forge create's first positional arg is a FILESYSTEM PATH, not a
    # remapping — so we point at the soldeer-installed dependency on disk rather
    # than the `@openzeppelin/contracts/...` import string.
    BSM_PROXY_ADDRESS=$(forge create \
        --rpc-url "$RPC_URL" \
        --private-key "$PRIVATE_KEY" \
        --broadcast \
        --root "$CONTRACTS_DIR" \
        "$CONTRACTS_DIR/dependencies/@openzeppelin-contracts-5.1.0/proxy/ERC1967/ERC1967Proxy.sol:ERC1967Proxy" \
        --constructor-args "$BSM_IMPL_ADDRESS" "$INIT_DATA" \
        2>&1 | grep -oE 'Deployed to: 0x[a-fA-F0-9]{40}' | tail -1 | awk '{print $3}')
    echo "AvatarBSM proxy deployed at: $BSM_PROXY_ADDRESS"
else
    echo "Stages 1+2 skipped — reusing existing BSM proxy at $BSM_PROXY_ADDRESS"
fi
echo ""

# ── Stage 3 — Patch deploy/definition.json with the BSM proxy address and call
# cargo-tangle's canonical deploy flow. The patched file is written to a temp
# path so the in-tree file stays untouched (its `manager: 0x0…0` is the template).
PATCHED_DEFINITION=$(mktemp --suffix=-avatar-blueprint.json)
trap 'rm -f "$PATCHED_DEFINITION"' EXIT
jq --arg mgr "$BSM_PROXY_ADDRESS" '.manager = $mgr' "$DEFINITION_FILE" > "$PATCHED_DEFINITION"

echo "Stage 3: cargo tangle blueprint deploy tangle …"
cargo tangle blueprint deploy tangle \
    --network testnet \
    --definition "$PATCHED_DEFINITION" \
    --http-rpc-url "$RPC_URL" \
    --ws-rpc-url "$WS_URL" \
    --tangle-contract "$TANGLE_CORE" \
    --keystore-path "$KEYSTORE_PATH"

echo ""
echo "=== Blueprint registered ==="
echo "AvatarBSM proxy: $BSM_PROXY_ADDRESS"
echo "(blueprint ID is logged by cargo-tangle above)"
