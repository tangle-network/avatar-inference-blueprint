// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;
import {Script, console2} from "forge-std/Script.sol";
import {AvatarBSM} from "../src/AvatarBSM.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

/// Model-specific manager deployment; cargo-tangle registers the definition.
contract Deploy is Script {
    function run() external {
        address registry = vm.envAddress("TANGLE_CORE");
        require(registry.code.length != 0, "TANGLE_CORE has no code");
        address token = vm.envAddress("PAYMENT_TOKEN");
        vm.startBroadcast();
        AvatarBSM implementation = new AvatarBSM();
        address manager =
            address(new ERC1967Proxy(address(implementation), abi.encodeCall(AvatarBSM.initialize, (token, registry))));
        vm.stopBroadcast();
        console2.log("DEPLOY_MANAGER=%s", vm.toString(manager));
    }
}
