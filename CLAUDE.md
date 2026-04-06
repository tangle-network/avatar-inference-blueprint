# CLAUDE.md

## Project Overview

Avatar Inference Blueprint for Tangle Network. Operators serve talking-head avatar generation (lip-synced video from audio + face image). Supports multiple backends: HeyGen (commercial), D-ID (commercial), Replicate (hosted open-source), ComfyUI (self-hosted SadTalker/MuseTalk).

## Architecture

Depends on [`tangle-inference-core`](../tangle-inference-core/) for shared billing, metrics, health, nonce store, x402 payment.

- **contracts/src/AvatarBSM.sol** — validates operator registration, per-second pricing, GPU validation for self-hosted backends
- **operator/src/avatar.rs** — unified `AvatarBackend` that dispatches to HeyGen/D-ID/Replicate/ComfyUI based on config
- **operator/src/server.rs** — async job model (POST → 202 Accepted + job_id, GET → poll)
- **operator/src/config.rs** — imports shared config from core, adds `AvatarConfig` (backend selection, pricing, API keys)
- **operator/src/lib.rs** — `AvatarInferenceServer` BackgroundService, on-chain job handler

## Build

```bash
cd contracts && forge build && forge test
cargo build -p avatar-inference
```

## Billing

Per-second pricing via `PerSecondCostModel`. Billing gate validates x402 SpendAuth upfront based on requested duration. Settlement adjusts to actual duration after generation completes.
