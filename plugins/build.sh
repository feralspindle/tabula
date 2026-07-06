#!/usr/bin/env bash
# Builds every plugin into a WASM component in plugins/dist/.
#
# Plugins target wasm32-unknown-unknown (NOT wasip2) so the resulting component
# has zero WASI imports — no ambient clock/RNG/IO exists for a plugin even in
# principle (CLAUDE.md invariant #4). `wasm-tools component new` then lifts the
# core module to a component against wit/tabula.wit.
set -euo pipefail

cd "$(dirname "$0")"
mkdir -p dist

for dir in */; do
    name="${dir%/}"
    [[ -f "$dir/Cargo.toml" ]] || continue
    echo "building plugin: $name"
    (cd "$name" && cargo build --release --target wasm32-unknown-unknown)
    core="$name/target/wasm32-unknown-unknown/release/${name//-/_}_plugin.wasm"
    if [[ ! -f "$core" ]]; then
        core="$name/target/wasm32-unknown-unknown/release/${name//-/_}.wasm"
    fi
    wasm-tools component new "$core" -o "dist/$name.wasm"
    wasm-tools validate "dist/$name.wasm"
    echo "  -> dist/$name.wasm"
done
