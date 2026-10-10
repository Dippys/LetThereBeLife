#!/usr/bin/env bash
# Full quality gate for bash/WSL. Usage: scripts/validate.sh [--quick] [--gpu]
# --quick skips the headless smoke run and the release success and slice tests. --gpu adds the hidden-window viewer smoke.
# Under WSL without a Linux toolchain, falls back to the Windows cargo.exe.
set -euo pipefail
cd "$(dirname "$0")/.."

quick=0 gpu=0
for arg in "$@"; do
    case "$arg" in
        --quick) quick=1 ;;
        --gpu) gpu=1 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

if command -v cargo >/dev/null 2>&1; then
    cargo=cargo
elif command -v cargo.exe >/dev/null 2>&1; then
    cargo=cargo.exe
elif [ -x "/mnt/c/Users/${USER}/.cargo/bin/cargo.exe" ]; then
    cargo="/mnt/c/Users/${USER}/.cargo/bin/cargo.exe"
else
    win_cargo=$(ls /mnt/c/Users/*/.cargo/bin/cargo.exe 2>/dev/null | head -n1 || true)
    [ -n "$win_cargo" ] || { echo "cargo not found" >&2; exit 1; }
    cargo="$win_cargo"
fi

echo "==> InitialDocumentation checksums"
(cd InitialDocumentation && sha256sum --quiet -c ../scripts/initial-documentation.sha256) ||
    { echo "InitialDocumentation/ was modified. It is read-only design input." >&2; exit 1; }
listed=$(wc -l < scripts/initial-documentation.sha256)
present=$(find InitialDocumentation -maxdepth 1 -type f | wc -l)
[ "$listed" -eq "$present" ] ||
    { echo "InitialDocumentation/ file count changed ($present vs $listed)." >&2; exit 1; }

echo "==> cargo fmt --check";  "$cargo" fmt --all -- --check
echo "==> cargo test";         "$cargo" test --workspace
echo "==> cargo clippy";       "$cargo" clippy --workspace --all-targets -- -D warnings
if [ "$quick" -eq 0 ]; then
    echo "==> Headless smoke"; "$cargo" run -p sim-headless -- --ticks 600 --seed 42
    echo "==> Definition of success and vertical slice (release)"
    "$cargo" test --release -p sim-headless --test success --test slice -- --ignored
fi
if [ "$gpu" -eq 1 ]; then
    echo "==> Viewer GPU smoke"; "$cargo" run -p sim-viewer -- --smoke-frames 2
fi
echo "All validation stages passed."
