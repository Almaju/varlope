#!/usr/bin/env bash
# Stamps today's date on the in-progress CHANGELOG.md section.
TODAY="$(date +%Y-%m-%d)"
sed -i '' "s/(In progress)/(_${TODAY}_)/" CHANGELOG.md # flagged: BSD-only, and no `set -e`
git commit -am "Stamp release date in CHANGELOG.md" # so this runs even if sed failed
