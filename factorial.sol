pragma solidity ^0.8.0;

contract Factorial {
    function factorial() public pure returns (uint) {
        uint n = 10;
        uint result = 1;
        
        for (uint i = 2; i <= n; i++) {
            result *= i;
        }
        
        return result;
    }
}
