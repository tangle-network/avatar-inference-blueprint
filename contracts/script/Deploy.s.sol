// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;
import {RegisterBlueprint} from "tangle-inference-core/script/RegisterBlueprint.s.sol";
import {console2} from "forge-std/Script.sol";
import {AvatarBSM} from "../src/AvatarBSM.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

contract Deploy is RegisterBlueprint {
    function _deployManager() internal override returns (address) {
        AvatarBSM impl = new AvatarBSM();
        console2.log("DEPLOY_AVATAR_BSM_IMPL=%s", vm.toString(address(impl)));
        return address(
            new ERC1967Proxy(address(impl), abi.encodeCall(AvatarBSM.initialize, (vm.envAddress("PAYMENT_TOKEN"))))
        );
    }

    function _report(address manager, uint64 blueprintId) internal override {
        console2.log("DEPLOY_AVATAR_BSM_PROXY=%s", vm.toString(manager));
        console2.log("DEPLOY_AVATAR_BLUEPRINT_ID=%s", vm.toString(blueprintId));
    }
}
