# EVM MLIR

[![Telegram Chat][tg-badge]][tg-url]
[![rust](https://github.com/lambdaclass/evm_mlir/actions/workflows/ci.yml/badge.svg)](https://github.com/lambdaclass/emv_mlir/actions/workflows/ci.yml)
[![license](https://img.shields.io/github/license/lambdaclass/evm_mlir)](/LICENSE)

[tg-badge]: https://img.shields.io/endpoint?url=https%3A%2F%2Ftg.sumanjay.workers.dev%2Fethereum_rust%2F&logo=telegram&label=chat&color=neon
[tg-url]: https://t.me/ethereum_rust

An EVM-bytecode to machine-bytecode compiler using MLIR and LLVM.

## Progress

<details>
<summary>Implemented opcodes (click to open)</summary>

1. (0x00) STOP
1. (0x01) ADD
1. (0x02) MUL
1. (0x03) SUB
1. (0x04) DIV
1. (0x05) SDIV
1. (0x06) MOD
1. (0x07) SMOD
1. (0x08) ADDMOD
1. (0x09) MULMOD
1. (0x0A) EXP
1. (0x0B) SIGNEXTEND
1. (0x10) LT
1. (0x11) GT
1. (0x12) SLT
1. (0x13) SGT
1. (0x14) EQ
1. (0x15) ISZERO
1. (0x16) AND
1. (0x17) OR
1. (0x18) XOR
1. (0x19) NOT
1. (0x1A) BYTE
1. (0x1B) SHL
1. (0x1C) SHR
1. (0x1D) SAR
1. (0x20) KECCAK256
1. (0x30) ADDRESS
1. (0x31) BALANCE
1. (0x32) ORIGIN
1. (0x33) CALLER
1. (0x34) CALLVALUE
1. (0x35) CALLDATALOAD
1. (0x36) CALLDATASIZE
1. (0x37) CALLDATACOPY
1. (0x38) CODESIZE
1. (0x39) CODECOPY
1. (0x3A) GASPRICE
1. (0x3B) EXTCODESIZE
1. (0x3C) EXTCODECOPY
1. (0x3F) EXTCODEHASH
1. (0x40) BLOCKHASH
1. (0x41) COINBASE
1. (0x42) TIMESTAMP
1. (0x43) NUMBER
1. (0x44) PREVRANDAO
1. (0x45) GASLIMIT
1. (0x46) CHAINID
1. (0x47) SELFBALANCE
1. (0x48) BASEFEE
1. (0x49) BLOBHASH
1. (0x4A) BLOBBASEFEE
1. (0x50) POP
1. (0x51) MLOAD
1. (0x52) MSTORE
1. (0x53) MSTORE8
1. (0x54) SLOAD
1. (0x55) SSTORE
1. (0x56) JUMP
1. (0x57) JUMPI
1. (0x58) PC
1. (0x59) MSIZE
1. (0x5A) GAS
1. (0x5B) JUMPDEST
1. (0x5E) MCOPY
1. (0x5F) PUSH0
1. (0x60) PUSH1
1. (0x61) PUSH2
1. (0x62) PUSH3
1. (0x63) PUSH4
1. (0x64) PUSH5
1. (0x65) PUSH6
1. (0x66) PUSH7
1. (0x67) PUSH8
1. (0x68) PUSH9
1. (0x69) PUSH10
1. (0x6A) PUSH11
1. (0x6B) PUSH12
1. (0x6C) PUSH13
1. (0x6D) PUSH14
1. (0x6E) PUSH15
1. (0x6F) PUSH16
1. (0x70) PUSH17
1. (0x71) PUSH18
1. (0x72) PUSH19
1. (0x73) PUSH20
1. (0x74) PUSH21
1. (0x75) PUSH22
1. (0x76) PUSH23
1. (0x77) PUSH24
1. (0x78) PUSH25
1. (0x79) PUSH26
1. (0x7A) PUSH27
1. (0x7B) PUSH28
1. (0x7C) PUSH29
1. (0x7D) PUSH30
1. (0x7E) PUSH31
1. (0x7F) PUSH32
1. (0x80) DUP1
1. (0x81) DUP2
1. (0x82) DUP3
1. (0x83) DUP4
1. (0x84) DUP5
1. (0x85) DUP6
1. (0x86) DUP7
1. (0x87) DUP8
1. (0x88) DUP9
1. (0x89) DUP10
1. (0x8A) DUP11
1. (0x8B) DUP12
1. (0x8C) DUP13
1. (0x8D) DUP14
1. (0x8E) DUP15
1. (0x8F) DUP16
1. (0x90) SWAP1
1. (0x91) SWAP2
1. (0x92) SWAP3
1. (0x93) SWAP4
1. (0x94) SWAP5
1. (0x95) SWAP6
1. (0x96) SWAP7
1. (0x97) SWAP8
1. (0x98) SWAP9
1. (0x99) SWAP10
1. (0x9A) SWAP11
1. (0x9B) SWAP12
1. (0x9C) SWAP13
1. (0x9D) SWAP14
1. (0x9E) SWAP15
1. (0x9F) SWAP16
1. (0xA0) LOG0
1. (0xA1) LOG1
1. (0xA2) LOG2
1. (0xA3) LOG3
1. (0xA4) LOG4
1. (0xF1) CALL
1. (0xF3) RETURN
1. (0xFD) REVERT
1. (0xFE) INVALID

</details>

<details>
<summary>Not yet implemented opcodes (click to open)</summary>

1. (0x3D) RETURNDATASIZE
1. (0x3E) RETURNDATACOPY
1. (0x5C) TLOAD
1. (0x5D) TSTORE
1. (0xF0) CREATE
1. (0xF2) CALLCODE
1. (0xF4) DELEGATECALL
1. (0xF5) CREATE2
1. (0xFA) STATICCALL
1. (0xFF) SELFDESTRUCT

</details>

## Getting Started

### Dependencies

- Linux or macOS (aarch64 included) only for now
- LLVM 18 with MLIR: On debian you can use [apt.llvm.org](https://apt.llvm.org/), on macOS you can use brew
- Rust
- Git

### Setup

> This step applies to all operating systems.

Run the following make target to install the dependencies (**both Linux and macOS**):

```bash
make deps
```

#### Linux

Since Linux distributions change widely, you need to install LLVM 18 via your package manager, compile it or check if the current release has a Linux binary.

If you are on Debian/Ubuntu, check out the repository https://apt.llvm.org/
Then you can install with:

```bash
sudo apt-get install llvm-18 llvm-18-dev llvm-18-runtime clang-18 clang-tools-18 lld-18 libpolly-18-dev libmlir-18-dev mlir-18-tools
```

If you decide to build from source, here are some indications:

<details><summary>Install LLVM from source instructions</summary>

```bash
# Go to https://github.com/llvm/llvm-project/releases
# Download the latest LLVM 18 release:
# The blob to download is called llvm-project-18.x.x.src.tar.xz

# For example
wget https://github.com/llvm/llvm-project/releases/download/llvmorg-18.1.4/llvm-project-18.1.4.src.tar.xz
tar xf llvm-project-18.1.4.src.tar.xz

cd llvm-project-18.1.4.src
mkdir build
cd build

# The following cmake command configures the build to be installed to /opt/llvm-18
cmake -G Ninja ../llvm \
   -DLLVM_ENABLE_PROJECTS="mlir;clang;clang-tools-extra;lld;polly" \
   -DLLVM_BUILD_EXAMPLES=OFF \
   -DLLVM_TARGETS_TO_BUILD="Native" \
   -DCMAKE_INSTALL_PREFIX=/opt/llvm-18 \
   -DCMAKE_BUILD_TYPE=RelWithDebInfo \
   -DLLVM_PARALLEL_LINK_JOBS=4 \
   -DLLVM_ENABLE_BINDINGS=OFF \
   -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DLLVM_ENABLE_LLD=ON \
   -DLLVM_ENABLE_ASSERTIONS=OFF

ninja install
```

</details>

Setup a environment variable called `MLIR_SYS_180_PREFIX`, `LLVM_SYS_180_PREFIX` and `TABLEGEN_180_PREFIX` pointing to the llvm directory:

```bash
# For Debian/Ubuntu using the repository, the path will be /usr/lib/llvm-18
export MLIR_SYS_180_PREFIX=/usr/lib/llvm-18
export LLVM_SYS_180_PREFIX=/usr/lib/llvm-18
export TABLEGEN_180_PREFIX=/usr/lib/llvm-18
```

Run the deps target to install the other dependencies.

```bash
make deps
```

#### MacOS

The makefile `deps` target (which you should have ran before) installs LLVM 18 with brew for you, afterwards you need to execute the `env-macos.sh` script to setup the environment.

```bash
source scripts/env-macos.sh
```

### Running

To run the compiler, call `cargo run` while passing it a file with the EVM bytecode to compile.
There are some example files under `programs/`, for example:

```bash
cargo run programs/push32.bytecode
```

You can also specify the optimization level:

```bash
cargo run programs/push32.bytecode 3  # ranges from 0 to 3
```

### Testing

To only run the ethereum foundation tests, run the command `make test-eth`. if you want to run the rest of the tests (those that are not the ethereum foundation tests) just run 
`make test`

## Debugging the compiler

### Compile a program

To generate the necessary artifacts, you need to run `cargo run <filepath>`, with `<filepath>` being the path to a file containing the EVM bytecode to compile.

Writing EVM bytecode directly can be a bit difficult, so you can edit [src/main.rs](../src/main.rs), modifying the `program` variable with the structure of your EVM program. After that you just run `cargo run`.

An example edit would look like this:

```rust
fn main() {
    let program = vec![
            Operation::Push0,
            Operation::PushN(BigUint::from(42_u8)),
            Operation::Add,
        ];
    // ...
}
```

### Inspecting the artifacts

The most useful ones to inspect are the MLIR-IR (`<name>.mlir`) and Assembly (`<name>.asm`) files. The first one has a one-to-one mapping with the operations added in the compiler, while the second one contains the instructions that are executed by your machine.

The other generated artifacts are:

- Semi-optimized MLIR-IR (`<name>.after-pass.mlir`)
- LLVM-IR (`<name>.ll`)
- Object file (`<name>.o`)
- Executable (`<name>`)

### Running with a debugger

Once we have the executable, we can run it with a debugger (here we use `lldb`, but you can use others). To run with `lldb`, use `lldb <name>`.

To run until we reach our main function, we can use:

```lldb
br set -n main
run
```

#### Running a single step

`thread step-inst`

#### Reading registers

All registers: `register read`

The `x0` register: `register read x0`

#### Reading memory

To inspect the memory at `<address>`: `memory read <address>`

To inspect the memory at the address given by the register `x0`: `memory read $x0`

#### Reading the EVM stack

To pretty-print the EVM stack at address `X`: `memory read -s32 -fu -c4 X`

Reference:

- The `-s32` flag groups the bytes in 32-byte chunks.
- The `-fu` flag interprets the chunks as unsigned integers.
- The `-c4` flag includes 4 chunks: the one at the given address plus the three next chunks.

#### Restarting the program

To restart the program, just use `run` again.
