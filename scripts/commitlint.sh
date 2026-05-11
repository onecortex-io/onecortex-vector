#!/usr/bin/env bash
set -euo pipefail

msg_file="${1:?commit message file required}"
msg=$(cat "$msg_file")

# Allow merge/revert commits; otherwise enforce conventional commits
merge_revert_pattern='^(Merge|Revert)'
conventional_pattern='^(feat|fix|docs|refactor|test|chore|build|ci)(\([a-z0-9-]+\))?!?: .+'

if grep -Eq "$merge_revert_pattern" <<<"$msg"; then
  exit 0
fi

if ! grep -Eq "$conventional_pattern" <<<"$msg"; then
  echo "Invalid commit message: $msg"
  echo "Expected format: type[scope]!: description"
  echo "Allowed types: feat, fix, docs, refactor, test, chore, build, ci"
  exit 1
fi
