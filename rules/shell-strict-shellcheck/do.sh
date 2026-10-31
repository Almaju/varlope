#!/usr/bin/env bash
# Stamps today's date on the in-progress CHANGELOG.md section.
set -euo pipefail
TODAY="$(date +%Y-%m-%d)"
sed "s/^\(# v[0-9.]*\) (In progress)/\1 (_${TODAY}_)/" \
  CHANGELOG.md > CHANGELOG.md.tmp
mv CHANGELOG.md.tmp CHANGELOG.md
git commit -am "Stamp release date in CHANGELOG.md"
