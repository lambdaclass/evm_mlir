pragma solidity ^0.8.0;

contract FactorialWithoutFunction {
    uint public result;

    constructor() {
        uint n = 10;
        result = 1;

        for (uint i = 2; i <= n; i++) {
            result *= i;
        }
    }
}
