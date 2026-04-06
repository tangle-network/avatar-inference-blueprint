// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import { EnumerableSet } from "@openzeppelin/contracts/utils/structs/EnumerableSet.sol";
import { Initializable } from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import { UUPSUpgradeable } from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import { BlueprintServiceManagerBase } from "tnt-core/BlueprintServiceManagerBase.sol";

/// @title AvatarBSM — Blueprint Service Manager for talking-head avatar generation.
/// @notice Manages operator registration with GPU validation, per-second pricing,
///         and duration limits. Supports multiple avatar backends (HeyGen, D-ID,
///         ComfyUI self-hosted) — the backend choice is operator-level config,
///         not on-chain.
contract AvatarBSM is Initializable, UUPSUpgradeable, BlueprintServiceManagerBase {
    using EnumerableSet for EnumerableSet.AddressSet;

    // ── Errors ──────────────────────────────────────────────────────────

    error InsufficientGpuCapability(uint32 required, uint32 actual);
    error DurationExceedsLimit(uint32 requested, uint32 maxAllowed);

    // ── Events ──────────────────────────────────────────────────────────

    event OperatorRegistered(address indexed operator, string backend, uint32 gpuCount, uint32 totalVramMib);
    event AvatarConfigured(uint64 pricePerSecond, uint32 maxDurationSeconds, uint32 minGpuVramMib);

    // ── Storage ─────────────────────────────────────────────────────────

    struct AvatarModelConfig {
        uint64 pricePerSecond;
        uint32 maxDurationSeconds;
        uint32 minGpuVramMib;
        bool enabled;
    }

    struct OperatorCapabilities {
        string backend;        // "heygen", "did", "replicate", "comfyui"
        uint32 gpuCount;
        uint32 totalVramMib;
        string endpoint;
        bool active;
    }

    AvatarModelConfig public avatarConfig;
    mapping(address => OperatorCapabilities) public operatorCaps;
    EnumerableSet.AddressSet private _operators;
    address public tsUSD;

    // ── Initialization ──────────────────────────────────────────────────

    function initialize(address _tsUSD) external initializer {
        __UUPSUpgradeable_init();
        tsUSD = _tsUSD;
        avatarConfig = AvatarModelConfig({
            pricePerSecond: 500_000,     // 0.50 tsUSD/second
            maxDurationSeconds: 300,     // 5 minutes
            minGpuVramMib: 0,            // 0 = no GPU required (API proxy operators)
            enabled: true
        });
    }

    function _authorizeUpgrade(address) internal override {}

    // ── Admin ───────────────────────────────────────────────────────────

    function configureAvatar(
        uint64 pricePerSecond,
        uint32 maxDurationSeconds,
        uint32 minGpuVramMib
    ) external onlyBlueprintOwner {
        avatarConfig = AvatarModelConfig({
            pricePerSecond: pricePerSecond,
            maxDurationSeconds: maxDurationSeconds,
            minGpuVramMib: minGpuVramMib,
            enabled: true
        });
        emit AvatarConfigured(pricePerSecond, maxDurationSeconds, minGpuVramMib);
    }

    // ── Lifecycle Hooks ─────────────────────────────────────────────────

    /// @param registrationInputs abi.encode(string backend, uint32 gpuCount, uint32 totalVramMib, string endpoint)
    function onRegister(
        address operator,
        bytes calldata registrationInputs
    ) external payable override onlyFromTangle {
        (
            string memory backend,
            uint32 gpuCount,
            uint32 totalVramMib,
            string memory endpoint
        ) = abi.decode(registrationInputs, (string, uint32, uint32, string));

        // GPU validation only for self-hosted backends
        if (avatarConfig.minGpuVramMib > 0 && totalVramMib < avatarConfig.minGpuVramMib) {
            revert InsufficientGpuCapability(avatarConfig.minGpuVramMib, totalVramMib);
        }

        operatorCaps[operator] = OperatorCapabilities({
            backend: backend,
            gpuCount: gpuCount,
            totalVramMib: totalVramMib,
            endpoint: endpoint,
            active: true
        });
        _operators.add(operator);

        emit OperatorRegistered(operator, backend, gpuCount, totalVramMib);
    }

    function onUnregister(
        address operator
    ) external override onlyFromTangle {
        operatorCaps[operator].active = false;
        _operators.remove(operator);
    }

    function onRequest(
        uint64,
        address,
        address[] calldata,
        bytes calldata,
        uint64,
        address,
        uint256
    ) external payable override onlyFromTangle {
        // Service requests are handled via HTTP (x402), not on-chain jobs.
    }

    // ── Views ───────────────────────────────────────────────────────────

    function getOperator(address operator) external view returns (OperatorCapabilities memory) {
        return operatorCaps[operator];
    }

    function getOperatorCount() external view returns (uint256) {
        return _operators.length();
    }

    function getOperatorAt(uint256 index) external view returns (address) {
        return _operators.at(index);
    }

    function isOperatorActive(address operator) external view returns (bool) {
        return operatorCaps[operator].active;
    }
}
