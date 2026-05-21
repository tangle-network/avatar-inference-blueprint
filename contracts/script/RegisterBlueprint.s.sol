// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import { Script, console2 } from "forge-std/Script.sol";
import { ERC1967Proxy } from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import { Types } from "tnt-core/libraries/Types.sol";
import { AvatarBSM } from "../src/AvatarBSM.sol";

/// @notice Minimal interface for Tangle blueprint registration.
interface ITangle {
    function createBlueprint(Types.BlueprintDefinition calldata def) external returns (uint64);
}

/// @title RegisterBlueprint
/// @notice Deploys AvatarBSM (impl + UUPS proxy + initialize) and registers
///         the avatar-inference blueprint on Tangle in a single broadcast.
/// @dev    Run via: `forge script contracts/script/RegisterBlueprint.s.sol
///         --rpc-url $RPC_URL --broadcast --slow`
///
///         Mirrors the single-broadcast pattern shipping in sibling blueprint
///         repos. AvatarBSM is UUPS-upgradeable: empty constructor +
///         `initialize(address paymentToken)` on the proxy.
contract RegisterBlueprint is Script {
    // ─────────────────────────────────────────────────────────────────────────
    // Defaults — overridable via env vars for non-anvil chains.
    // ─────────────────────────────────────────────────────────────────────────

    // Anvil well-known deployer key (default when no PRIVATE_KEY env is set).
    uint256 constant DEFAULT_DEPLOYER_KEY =
        0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80;

    // Tangle protocol address on a LocalTestnet anvil snapshot. For real
    // chains (Base Sepolia, mainnet) pass TANGLE_CORE via env.
    address constant DEFAULT_TANGLE = 0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9;

    // USDC on Base Sepolia. The avatar operator settles in this token under
    // the shielded billing flow. For other networks pass PAYMENT_TOKEN via env.
    address constant DEFAULT_PAYMENT_TOKEN = 0x036CbD53842c5426634e7929541eC2318f3dCF7e;

    function run() external {
        uint256 deployerKey = vm.envOr("PRIVATE_KEY", DEFAULT_DEPLOYER_KEY);
        address tangleAddr = vm.envOr("TANGLE_CORE", DEFAULT_TANGLE);
        address paymentToken = vm.envOr("PAYMENT_TOKEN", DEFAULT_PAYMENT_TOKEN);

        ITangle tangle = ITangle(tangleAddr);

        vm.startBroadcast(deployerKey);

        // ── Deploy AvatarBSM (UUPS impl + proxy + initialize) ───────────────
        AvatarBSM impl = new AvatarBSM();
        ERC1967Proxy proxy = new ERC1967Proxy(
            address(impl),
            abi.encodeCall(AvatarBSM.initialize, (paymentToken))
        );
        AvatarBSM bsm = AvatarBSM(payable(address(proxy)));

        // ── Register on Tangle ──────────────────────────────────────────────
        uint64 blueprintId = tangle.createBlueprint(_buildDefinition(address(bsm)));

        vm.stopBroadcast();

        // ── Output for bash wrapper parsing ─────────────────────────────────
        console2.log("DEPLOY_AVATAR_BSM_IMPL=%s", vm.toString(address(impl)));
        console2.log("DEPLOY_AVATAR_BSM_PROXY=%s", vm.toString(address(bsm)));
        console2.log("DEPLOY_AVATAR_BLUEPRINT_ID=%s", vm.toString(blueprintId));
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Blueprint Definition builder — sourced from deploy/definition.json
    // ═════════════════════════════════════════════════════════════════════════

    function _buildDefinition(address manager) internal pure returns (Types.BlueprintDefinition memory def) {
        def.metadataUri = "https://github.com/tangle-network/avatar-inference-blueprint";
        // metadataHash is a digest of the canonical metadata JSON. Until that
        // payload is pinned via IPFS, derive it from the metadataUri so the
        // value is deterministic + traceable.
        def.metadataHash = keccak256(bytes(def.metadataUri));
        def.manager = manager;
        def.masterManagerRevision = 0;
        def.hasConfig = true;

        // Event-driven pricing: operators are paid per avatar render rather
        // than on a fixed subscription cadence. AvatarBSM enforces
        // per-second pricing + duration limits at the contract layer.
        def.config = Types.BlueprintConfig({
            membership: Types.MembershipModel.Dynamic,
            pricing: Types.PricingModel.EventDriven,
            minOperators: 1,
            maxOperators: 0, // unbounded
            subscriptionRate: 0,
            subscriptionInterval: 0,
            eventRate: 0 // operators negotiate price per call via RFQ
        });

        def.metadata = Types.BlueprintMetadata({
            name: "Avatar Inference Blueprint",
            description: "AI avatar generation and talking-head synthesis operator via Tangle",
            author: "Tangle Network",
            category: "AI/Inference",
            codeRepository: "https://github.com/tangle-network/avatar-inference-blueprint",
            logo: "",
            website: "https://tangle.tools",
            license: "MIT OR Apache-2.0",
            profilingData: "{\"execution_profile\":{\"gpu\":{\"policy\":\"required\",\"min_count\":1,\"min_vram_gb\":16}}}"
        });

        def.jobs = _buildJobs();

        def.registrationSchema = "";
        def.requestSchema = "";

        def.sources = new Types.BlueprintSource[](1);
        Types.BlueprintBinary[] memory bins = new Types.BlueprintBinary[](1);
        bins[0] = Types.BlueprintBinary({
            arch: Types.BlueprintArchitecture.Amd64,
            os: Types.BlueprintOperatingSystem.Linux,
            name: "avatar-operator",
            // Placeholder until release artifacts are published + pinned. Same
            // pattern used by the sibling inference blueprint script.
            sha256: bytes32(uint256(0xdeadbeef))
        });
        def.sources[0] = Types.BlueprintSource({
            kind: Types.BlueprintSourceKind.Native,
            container: Types.ImageRegistrySource("", "", ""),
            wasm: Types.WasmSource(Types.WasmRuntime.Unknown, Types.BlueprintFetcherKind.None, "", ""),
            native: Types.NativeSource(
                Types.BlueprintFetcherKind.None,
                "file:///target/release/avatar-operator",
                "./target/release/avatar-operator"
            ),
            testing: Types.TestingSource("avatar-inference", "avatar-operator", "."),
            binaries: bins
        });

        // Per deploy/definition.json: avatar supports both Dynamic and Fixed.
        def.supportedMemberships = new Types.MembershipModel[](2);
        def.supportedMemberships[0] = Types.MembershipModel.Dynamic;
        def.supportedMemberships[1] = Types.MembershipModel.Fixed;
    }

    function _buildJobs() internal pure returns (Types.JobDefinition[] memory jobs) {
        jobs = new Types.JobDefinition[](1);
        // Job 0: generate_avatar
        //   params: (string backend, string prompt, string voicePresetOrUrl,
        //            uint32 durationSeconds, uint32 maxWaitMs)
        //   result: (bytes video, string mimeType, uint32 widthPx, uint32 heightPx)
        // The Rust operator enforces these shapes; on-chain schemas are kept
        // empty to match the pattern used by sibling blueprint scripts.
        // tnt-core's SchemaLib can be wired in a follow-up once it stabilises.
        jobs[0] = Types.JobDefinition({
            name: "generate_avatar",
            description: "Render a talking-head avatar from text + voice via the operator backend",
            metadataUri: "",
            paramsSchema: "",
            resultSchema: ""
        });
    }
}
