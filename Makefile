.PHONY: check-deps deps lint fmt test usage

#
# Environment detection.
#

UNAME := $(shell uname)

usage:
	@echo "Usage:"
	@echo "    deps:		 Installs the necesarry dependencies."
	@echo "    test:         Runs all tests."
	@echo "    fmt:          Formats all files."
	@echo "    lint:         Checks format and runs lints."

check-deps:
	ifeq (, $(shell which cargo))
		$(error "The cargo command could not be found in your PATH, please install Rust: https://www.rust-lang.org/tools/install")
	endif
	ifndef LLVM_SYS_180_PREFIX
		$(error Could not find a suitable LLVM 18 toolchain, please set LLVM_SYS_180_PREFIX env pointing to the LLVM 18 dir)
	endif
	ifndef MLIR_SYS_180_PREFIX
		$(error Could not find a suitable LLVM 18 toolchain (mlir), please set MLIR_SYS_180_PREFIX env pointing to the LLVM 18 dir)
	endif
	ifndef TABLEGEN_180_PREFIX
		$(error Could not find a suitable LLVM 18 toolchain (tablegen), please set TABLEGEN_180_PREFIX env pointing to the LLVM 18 dir)
	endif
		@echo "[make] LLVM is correctly set at $(MLIR_SYS_180_PREFIX)."

deps:
ifeq ($(UNAME), Linux)
deps:
endif
ifeq ($(UNAME), Darwin)
deps: deps-macos
endif

deps-macos:
	-brew install llvm@18 --quiet
	@echo "You need to run source scripts/env-macos.sh to setup the environment."

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-features --benches --examples --tests -- -D warnings

fmt:
	cargo fmt --all

test:
	cargo nextest run --workspace --all-features


###### Ethereum tests ######

ETHTEST_SHASUM := ".ethtest_shasum"
ETHTEST_VERSION := $(shell cat .ethtest_version)
ETHTEST_TAR := "ethtests-${ETHTEST_VERSION}.tar.gz"

${ETHTEST_TAR}: .ethtest_version
	curl -Lo ${ETHTEST_TAR} https://github.com/ethereum/tests/archive/refs/tags/${ETHTEST_VERSION}.tar.gz

ethtests: ${ETHTEST_TAR}
	mkdir -p "$@"
	tar -xzmf "$<" --strip-components=1 -C "$@"
	@cat ${ETHTEST_SHASUM}
	sha256sum -c ${ETHTEST_SHASUM}
