#!/usr/bin/env bash
# Register the avatar-inference blueprint on Tangle.
#
# Single-shot flow: deploys AvatarBSM (impl + UUPS proxy + initialize)
# AND calls Tangle.createBlueprint in the same broadcast via
# `contracts/script/RegisterBlueprint.s.sol`. This replaces the prior
# cargo-tangle CLI invocation, matching the pattern shipping across the
# other Tangle blueprint repos.
#
# AvatarBSM is a UUPS-upgradeable contract (extends Initializable +
# UUPSUpgradeable + BlueprintServiceManagerBase), so its `manager` address
# must be the proxy, not the implementation. The constructor is empty (no
# args); the payment token is wired via initialize(address) on the proxy.
#
# Prerequisites:
#   - forge (Foundry) installed
#   - Deployer wallet funded on the target network
#
# Usage (Base Sepolia, against the already-deployed Tangle protocol):
#
#   export PRIVATE_KEY=0x...
#   export RPC_URL=https://sepolia.base.org
#   export TANGLE_CORE=0xC9b0716a187072be0f38A5D972392C6479b9Cfe3
#   export PAYMENT_TOKEN=0x036CbD53842c5426634e7929541eC2318f3dCF7e  # USDC sepolia
#   ./deploy/register-blueprint.sh
#
# Local anvil (LocalTestnet snapshot):
#
#   export RPC_URL=http://127.0.0.1:8545
#   ./deploy/register-blueprint.sh   # uses anvil deployer key + Tangle/USDC defaults
#
# Optional overrides (operator-side registration only — they get echoed into
# the abi-encoded registration inputs the operator uses later):
#   BACKEND          Avatar backend (default: comfyui)
#   GPU_COUNT        GPUs the operator exposes (default: 1)
#   TOTAL_VRAM       Total VRAM in MiB (default: 24000)
#   ENDPOINT         Operator HTTP endpoint (default: https://your-operator.example.com)
#
# Outputs (parsed by deployment scripts, do not change without coordinating):
#   DEPLOY_AVATAR_BSM_IMPL=<address>
#   DEPLOY_AVATAR_BSM_PROXY=<address>
#   DEPLOY_AVATAR_BLUEPRINT_ID=<u64>

set -euo pipefail

: "${RPC_URL:?Set RPC_URL}"
: "${PRIVATE_KEY:?Set PRIVATE_KEY}"

BACKEND="${BACKEND:-comfyui}"
GPU_COUNT="${GPU_COUNT:-1}"
TOTAL_VRAM="${TOTAL_VRAM:-24000}"
ENDPOINT="${ENDPOINT:-https://your-operator.example.com}"

echo "=== Avatar-Inference Blueprint Registration ==="
echo "Network:     $(cast chain-id --rpc-url "$RPC_URL")"
echo "Deployer:    $(cast wallet address --private-key "$PRIVATE_KEY")"
echo "Tangle Core: ${TANGLE_CORE:-<default from RegisterBlueprint.s.sol>}"
echo "Payment:     ${PAYMENT_TOKEN:-<default USDC sepolia>}"
echo "Backend:     $BACKEND"
echo "GPUs:        $GPU_COUNT (${TOTAL_VRAM} MiB)"
echo "Endpoint:    $ENDPOINT"
echo ""

cd "$(dirname "$0")/../contracts"

# Deploy BSM (impl + proxy + initialize) AND register the blueprint in one
# forge-script broadcast.
DEPLOY_OUTPUT=$(PRIVATE_KEY="$PRIVATE_KEY" \
    TANGLE_CORE="${TANGLE_CORE:-}" \
    PAYMENT_TOKEN="${PAYMENT_TOKEN:-}" \
    forge script script/RegisterBlueprint.s.sol \
        --rpc-url "$RPC_URL" \
        --broadcast --slow)

echo "$DEPLOY_OUTPUT"

# Extract the BSM proxy address + blueprint ID for downstream scripts.
BSM_ADDRESS=$(echo "$DEPLOY_OUTPUT" | grep -oE 'DEPLOY_AVATAR_BSM_PROXY=0x[0-9a-fA-F]+' | tail -1 | cut -d= -f2)
BLUEPRINT_ID=$(echo "$DEPLOY_OUTPUT" | grep -oE 'DEPLOY_AVATAR_BLUEPRINT_ID=[0-9]+' | tail -1 | cut -d= -f2)

if [ -z "$BSM_ADDRESS" ] || [ -z "$BLUEPRINT_ID" ]; then
    echo "ERROR: failed to extract addresses from forge output"
    exit 1
fi

echo ""
echo "=== Blueprint registered ==="
echo "Blueprint ID:     $BLUEPRINT_ID"
echo "AvatarBSM proxy:  $BSM_ADDRESS"
echo ""

# Operator registration is a separate step (per-operator). Encode the
# registration inputs so the operator can call Tangle.registerOperator
# with the right calldata. AvatarBSM.onRegister decodes:
#   (string backend, uint32 gpuCount, uint32 totalVramMib, string endpoint)
REG_INPUTS=$(cast abi-encode \
    "f(string,uint32,uint32,string)" \
    "$BACKEND" "$GPU_COUNT" "$TOTAL_VRAM" "$ENDPOINT")

echo "Operator registration inputs (use these to register an operator):"
echo "  $REG_INPUTS"
echo ""
echo "To register an operator now:"
echo "  cast send ${TANGLE_CORE:-<TANGLE_CORE>} \\"
echo "    'registerOperator(uint64,bytes)' $BLUEPRINT_ID $REG_INPUTS \\"
echo "    --rpc-url $RPC_URL --private-key \$OPERATOR_KEY"
