#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")" || exit 1

echo "nanobot-rs line count"
echo "====================="
echo ""

for dir in crates/*; do
  [ -d "$dir" ] || continue
  name=$(basename "$dir")
  count=$(find "$dir" -type f -name "*.rs" -exec cat {} + | wc -l | tr -d ' ')
  printf "  %-24s %5s lines\n" "$name/" "$count"
done

echo ""
total=$(find crates -type f -name "*.rs" -exec cat {} + | wc -l | tr -d ' ')
echo "  Rust total:     $total lines"
