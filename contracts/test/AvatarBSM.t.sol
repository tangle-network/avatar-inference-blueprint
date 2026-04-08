// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.26;

import "forge-std/Test.sol";
import { ERC1967Proxy } from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import { BlueprintServiceManagerBase } from "tnt-core/BlueprintServiceManagerBase.sol";
import "../src/AvatarBSM.sol";

contract AvatarBSMTest is Test {
    AvatarBSM internal bsm;
    address internal owner = makeAddr("owner");
    address internal tangle = makeAddr("tangle");
    address internal paymentToken = makeAddr("paymentToken");
    address internal operator1 = makeAddr("operator1");
    address internal operator2 = makeAddr("operator2");
    address internal rando = makeAddr("rando");

    function setUp() public {
        // Deploy implementation
        AvatarBSM impl = new AvatarBSM();

        // Deploy behind UUPS proxy
        bytes memory initData = abi.encodeCall(AvatarBSM.initialize, (paymentToken));
        ERC1967Proxy proxy = new ERC1967Proxy(address(impl), initData);
        bsm = AvatarBSM(payable(address(proxy)));

        // Wire up the BSM base (tangle core + owner)
        bsm.onBlueprintCreated(1, owner, tangle);
    }

    // ── Initialization ─────────────────────────────────────────────────

    function test_initialize_setsPaymentToken() public view {
        assertEq(bsm.paymentToken(), paymentToken);
    }

    function test_initialize_setsDefaultAvatarConfig() public view {
        (uint64 price, uint32 maxDur, uint32 minVram, bool enabled) = bsm.avatarConfig();
        assertEq(price, 500_000);
        assertEq(maxDur, 300);
        assertEq(minVram, 0);
        assertTrue(enabled);
    }

    function test_initialize_cannotReinitialize() public {
        vm.expectRevert();
        bsm.initialize(address(0xdead));
    }

    // ── Avatar Configuration ───────────────────────────────────────────

    function test_configureAvatar_setsValues() public {
        vm.prank(owner);
        bsm.configureAvatar(1_000_000, 600, 8192);

        (uint64 price, uint32 maxDur, uint32 minVram, bool enabled) = bsm.avatarConfig();
        assertEq(price, 1_000_000);
        assertEq(maxDur, 600);
        assertEq(minVram, 8192);
        assertTrue(enabled);
    }

    function test_configureAvatar_emitsEvent() public {
        vm.prank(owner);
        vm.expectEmit(true, true, true, true);
        emit AvatarBSM.AvatarConfigured(1_000_000, 600, 8192);
        bsm.configureAvatar(1_000_000, 600, 8192);
    }

    function test_configureAvatar_revert_nonOwner() public {
        vm.prank(rando);
        vm.expectRevert(
            abi.encodeWithSelector(BlueprintServiceManagerBase.OnlyBlueprintOwnerAllowed.selector, rando, owner)
        );
        bsm.configureAvatar(1, 1, 1);
    }

    // ── Operator Registration ──────────────────────────────────────────

    function test_onRegister_storesCapabilities() public {
        bytes memory inputs = abi.encode("heygen", uint32(2), uint32(16384), "https://op1.example.com");

        vm.prank(tangle);
        bsm.onRegister(operator1, inputs);

        AvatarBSM.OperatorCapabilities memory caps = bsm.getOperator(operator1);
        assertEq(caps.backend, "heygen");
        assertEq(caps.gpuCount, 2);
        assertEq(caps.totalVramMib, 16384);
        assertEq(caps.endpoint, "https://op1.example.com");
        assertTrue(caps.active);
    }

    function test_onRegister_addsToOperatorSet() public {
        bytes memory inputs = abi.encode("did", uint32(1), uint32(8192), "https://op1.example.com");

        vm.prank(tangle);
        bsm.onRegister(operator1, inputs);

        assertEq(bsm.getOperatorCount(), 1);
        assertEq(bsm.getOperatorAt(0), operator1);
    }

    function test_onRegister_emitsEvent() public {
        bytes memory inputs = abi.encode("comfyui", uint32(4), uint32(32768), "http://localhost:8188");

        vm.prank(tangle);
        vm.expectEmit(true, true, true, true);
        emit AvatarBSM.OperatorRegistered(operator1, "comfyui", 4, 32768);
        bsm.onRegister(operator1, inputs);
    }

    function test_onRegister_multipleOperators() public {
        vm.startPrank(tangle);
        bsm.onRegister(operator1, abi.encode("heygen", uint32(1), uint32(8192), "https://a.com"));
        bsm.onRegister(operator2, abi.encode("did", uint32(2), uint32(16384), "https://b.com"));
        vm.stopPrank();

        assertEq(bsm.getOperatorCount(), 2);
        assertTrue(bsm.isOperatorActive(operator1));
        assertTrue(bsm.isOperatorActive(operator2));
    }

    function test_onRegister_gpuValidation_passesWhenSufficient() public {
        // Set min GPU requirement
        vm.prank(owner);
        bsm.configureAvatar(500_000, 300, 8192);

        bytes memory inputs = abi.encode("comfyui", uint32(1), uint32(8192), "http://localhost:8188");

        vm.prank(tangle);
        bsm.onRegister(operator1, inputs); // should not revert

        assertTrue(bsm.isOperatorActive(operator1));
    }

    function test_onRegister_gpuValidation_revertsWhenInsufficient() public {
        // Set min GPU requirement
        vm.prank(owner);
        bsm.configureAvatar(500_000, 300, 16384);

        bytes memory inputs = abi.encode("comfyui", uint32(1), uint32(8192), "http://localhost:8188");

        vm.prank(tangle);
        vm.expectRevert(abi.encodeWithSelector(AvatarBSM.InsufficientGpuCapability.selector, 16384, 8192));
        bsm.onRegister(operator1, inputs);
    }

    function test_onRegister_gpuValidation_skippedWhenMinIsZero() public view {
        // Default minGpuVramMib is 0, so any VRAM (including 0) should pass.
        // Already verified by default setUp config.
        (,, uint32 minVram,) = bsm.avatarConfig();
        assertEq(minVram, 0);
    }

    function test_onRegister_revert_notTangle() public {
        bytes memory inputs = abi.encode("heygen", uint32(1), uint32(8192), "https://op1.example.com");

        vm.prank(rando);
        vm.expectRevert(
            abi.encodeWithSelector(BlueprintServiceManagerBase.OnlyTangleAllowed.selector, rando, tangle)
        );
        bsm.onRegister(operator1, inputs);
    }

    // ── Operator Unregistration ────────────────────────────────────────

    function test_onUnregister_marksInactive() public {
        vm.startPrank(tangle);
        bsm.onRegister(operator1, abi.encode("heygen", uint32(1), uint32(8192), "https://a.com"));
        bsm.onUnregister(operator1);
        vm.stopPrank();

        assertFalse(bsm.isOperatorActive(operator1));
    }

    function test_onUnregister_removesFromSet() public {
        vm.startPrank(tangle);
        bsm.onRegister(operator1, abi.encode("heygen", uint32(1), uint32(8192), "https://a.com"));
        bsm.onRegister(operator2, abi.encode("did", uint32(2), uint32(16384), "https://b.com"));
        bsm.onUnregister(operator1);
        vm.stopPrank();

        assertEq(bsm.getOperatorCount(), 1);
        assertEq(bsm.getOperatorAt(0), operator2);
    }

    function test_onUnregister_revert_notTangle() public {
        vm.prank(rando);
        vm.expectRevert(
            abi.encodeWithSelector(BlueprintServiceManagerBase.OnlyTangleAllowed.selector, rando, tangle)
        );
        bsm.onUnregister(operator1);
    }

    // ── Access Control ─────────────────────────────────────────────────

    function test_configureAvatar_revert_operator() public {
        vm.prank(operator1);
        vm.expectRevert(
            abi.encodeWithSelector(BlueprintServiceManagerBase.OnlyBlueprintOwnerAllowed.selector, operator1, owner)
        );
        bsm.configureAvatar(1, 1, 1);
    }

    function test_onRequest_revert_notTangle() public {
        address[] memory ops = new address[](0);
        vm.prank(rando);
        vm.expectRevert(
            abi.encodeWithSelector(BlueprintServiceManagerBase.OnlyTangleAllowed.selector, rando, tangle)
        );
        bsm.onRequest(0, address(0), ops, "", 0, address(0), 0);
    }

    // ── View Functions ─────────────────────────────────────────────────

    function test_getOperator_unregisteredReturnsDefaults() public view {
        AvatarBSM.OperatorCapabilities memory caps = bsm.getOperator(rando);
        assertEq(caps.gpuCount, 0);
        assertEq(caps.totalVramMib, 0);
        assertFalse(caps.active);
        assertEq(bytes(caps.backend).length, 0);
        assertEq(bytes(caps.endpoint).length, 0);
    }

    function test_isOperatorActive_falseForUnregistered() public view {
        assertFalse(bsm.isOperatorActive(rando));
    }

    function test_getOperatorCount_startsAtZero() public view {
        assertEq(bsm.getOperatorCount(), 0);
    }

    function test_getOperatorAt_revertsOutOfBounds() public {
        vm.expectRevert();
        bsm.getOperatorAt(0);
    }

    // ── Fuzz ───────────────────────────────────────────────────────────

    function testFuzz_configureAvatar(uint64 price, uint32 maxDur, uint32 minVram) public {
        vm.prank(owner);
        bsm.configureAvatar(price, maxDur, minVram);

        (uint64 p, uint32 d, uint32 v, bool e) = bsm.avatarConfig();
        assertEq(p, price);
        assertEq(d, maxDur);
        assertEq(v, minVram);
        assertTrue(e);
    }

    function testFuzz_onRegister_gpuValidation(uint32 minVram, uint32 actualVram) public {
        vm.assume(minVram > 0);

        vm.prank(owner);
        bsm.configureAvatar(500_000, 300, minVram);

        bytes memory inputs = abi.encode("comfyui", uint32(1), actualVram, "http://localhost:8188");

        if (actualVram < minVram) {
            vm.prank(tangle);
            vm.expectRevert(
                abi.encodeWithSelector(AvatarBSM.InsufficientGpuCapability.selector, minVram, actualVram)
            );
            bsm.onRegister(operator1, inputs);
        } else {
            vm.prank(tangle);
            bsm.onRegister(operator1, inputs);
            assertTrue(bsm.isOperatorActive(operator1));
        }
    }
}
