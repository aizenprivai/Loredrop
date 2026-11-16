#!/usr/bin/env bash
# Builds the calculator app and writes numworks/loredrop.nwa.
#
# Needs: Rust with the `thumbv7em-none-eabihf` target
#   (rustup target add thumbv7em-none-eabihf), and llvm-objcopy (or
#   arm-none-eabi-objcopy) to drop debug data and LTO bitcode from the file.
# Optional: Node.js, to check the result with NumWorks' own linker (nwlink).
set -euo pipefail
cd "$(dirname "$0")"

(cd app && cargo build --release)
OBJCOPY=$(command -v llvm-objcopy || command -v arm-none-eabi-objcopy)
"$OBJCOPY" --strip-debug --remove-section=.llvmbc app/target/thumbv7em-none-eabihf/release/loredrop loredrop.nwa
ls -l loredrop.nwa

# The N0110's FPU has no double precision: refuse any f64 instruction.
OBJDUMP=$(command -v llvm-objdump || command -v arm-none-eabi-objdump || true)
if [ -n "$OBJDUMP" ]; then
    if "$OBJDUMP" -d loredrop.nwa | grep -qE '\.f64'; then
        echo "ERROR: double-precision instruction found (would crash on the N0110)" >&2
        exit 1
    fi
    echo "no double-precision instructions: OK"
fi

if command -v npx > /dev/null; then
    # Link it the way the calculator will (default memory layout) to catch
    # missing symbols or an app that doesn't fit.
    npx --yes -- nwlink@1.0.0 nwa-elf loredrop.nwa "${TMPDIR:-/tmp}/loredrop-check.elf"
    echo "nwlink check passed: $(npx --yes -- nwlink@1.0.0 nwa-name loredrop.nwa)"
fi
