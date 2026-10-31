#!/usr/bin/env bash
# Fail if varlope depends on any crate (name + version) that is not already
# in mozilla/application-services' Cargo.lock.
set -euo pipefail

url=https://raw.githubusercontent.com/mozilla/application-services/main/Cargo.lock
packages() { awk '/^name = /{n=$3} /^version = /{print n" "$3}' | sort -u; }

theirs=$(curl -fsSL "$url" | packages)
ours=$(packages < Cargo.lock | grep -v '^"varlope" ')
missing=$(comm -23 <(echo "$ours") <(echo "$theirs"))
if [ -n "$missing" ]; then
    echo "Not in application-services' Cargo.lock:"
    echo "$missing"
    exit 1
fi
echo "All $(echo "$ours" | wc -l) dependencies are in application-services' Cargo.lock."
