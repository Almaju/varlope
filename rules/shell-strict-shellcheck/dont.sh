#!/usr/bin/env bash
# Stamps today's date on the in-progress CHANGELOG.md section.
# flagged (line 1): no `set -euo pipefail`; the commit runs even if sed fails
TODAY="$(date +%Y-%m-%d)"
sed -i '' "s/^\(# v[0-9.]*\) (In progress)/\1 (_${TODAY}_)/" \
  CHANGELOG.md # flagged: `sed -i ''` is BSD-only; GNU sed rejects it
git commit -am "Stamp release date in CHANGELOG.md"
