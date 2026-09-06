# Avatar Inference Blueprint

Operators generate avatar video from audio and an image.
Read [README.md](README.md) for setup and [operator/Cargo.toml](operator/Cargo.toml) for supported dependencies.

For provider changes, inspect [avatar.rs](operator/src/avatar.rs) and [config.rs](operator/src/config.rs).
For the asynchronous submission and polling contract, inspect [server.rs](operator/src/server.rs).
Keep common payment validation, health, and metrics in `tangle-inference-core`.

Billing estimates compute time before generation and settles after completion.
Check the server's cost calculation and shared settlement behavior before changing pricing; output duration is not compute duration.
Verify submission, polling, failure, and settlement through the actual server, replacing only unavailable external providers.
Exercise registration and pricing changes against [AvatarBSM.sol](contracts/src/AvatarBSM.sol) with contract tests.
