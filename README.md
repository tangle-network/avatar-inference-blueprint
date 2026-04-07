![Tangle Network Banner](https://raw.githubusercontent.com/tangle-network/tangle/refs/heads/main/assets/Tangle%20%20Banner.png)

<h1 align="center">Avatar Inference Blueprint</h1>

<p align="center"><em>Talking-head avatar generation on <a href="https://tangle.tools">Tangle</a> — lip-synced video from audio + face image, paid anonymously via shielded credits.</em></p>

<p align="center">
  <a href="https://discord.com/invite/cv8EfJu3Tn"><img src="https://img.shields.io/discord/833784453251596298?label=Discord" alt="Discord"></a>
  <a href="https://t.me/tanglenet"><img src="https://img.shields.io/endpoint?color=neon&url=https%3A%2F%2Ftg.sumanjay.workers.dev%2Ftanglenet" alt="Telegram"></a>
</p>

## Overview

A Tangle Blueprint enabling operators to serve talking-head avatar generation with anonymous payments through the [Shielded Payment Gateway](https://github.com/tangle-network/shielded-payment-gateway).

**Backends (operator chooses via config):**
- **HeyGen** — commercial API, best-in-class quality (Avatar IV)
- **D-ID** — commercial API, budget option
- **Replicate** — hosted open-source models (SadTalker)
- **ComfyUI** — self-hosted (SadTalker/MuseTalk nodes, true decentralization)

**Async job model:**
- `POST /v1/avatar/generate` → 202 Accepted + job_id
- `GET /v1/avatar/jobs/:id` → poll for completion

Per-second billing via x402 SpendAuth. Built with [Blueprint SDK](https://github.com/tangle-network/blueprint) and [tangle-inference-core](https://github.com/tangle-network/tangle-inference-core).

## Components

| Component | Language | Description |
|-----------|----------|-------------|
| `operator/` | Rust | Operator binary — multi-backend avatar proxy, HTTP server, SpendAuth billing |
| `contracts/` | Solidity | AvatarBSM — GPU validation for self-hosted, per-second pricing |

## Quick Start

```bash
# Build operator
cargo build -p avatar-inference

# Build contracts
cd contracts && forge build

# Configure (set your backend)
export AVATAR_OP__AVATAR__BACKEND=heygen
export AVATAR_OP__AVATAR__HEYGEN_API_KEY=your_key_here

# Run
./target/debug/avatar-operator
```

## Related Repos

- [tangle-inference-core](https://github.com/tangle-network/tangle-inference-core) — shared billing, metrics, health
- [shielded-payment-gateway](https://github.com/tangle-network/shielded-payment-gateway) — the payment layer
- [blueprint](https://github.com/tangle-network/blueprint) — Blueprint SDK
- [tnt-core](https://github.com/tangle-network/tnt-core) — Tangle core protocol

## License

MIT
