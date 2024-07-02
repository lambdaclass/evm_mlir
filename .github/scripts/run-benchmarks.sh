#!/usr/bin/env bash


echo "# Benchmarking results" > bench-hyperfine.md
sudo swapoff -a # Disabling swap memory to reduce noice
for program in factorial fibonacci;
do
    hyperfine -w 5 -r 20 -N --export-markdown "bench-${program}.md" \
        -n "evm_mlir_${program}" "target/release/evm_mlir_${program} 100000 1000" \
        -n "revm_${program}" "target/release/revm_${program} 100000 1000"

    {
        echo "## Benchmark for program \`$program\`"
        echo
        echo "<details><summary>Open benchmarks</summary>"
        echo
        echo "<br>"
        echo
        cat "bench-${program}.md"
        echo
        echo "</details>"
        echo
    } >> bench-hyperfine.md

done

