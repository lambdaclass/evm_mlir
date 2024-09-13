pragma solidity ^0.8.0;

contract Fibonacci {
    // Función para calcular el número de Fibonacci n-ésimo de manera iterativa
    function fibonacci(uint n) public pure returns (uint) {
        if (n == 0) {
            return 0;
        } else if (n == 1) {
            return 1;
        }

        uint a = 0;
        uint b = 1;
        uint result;

        for (uint i = 2; i <= n; i++) {
            result = a + b;
            a = b;
            b = result;
        }

        return result;
    }
}
