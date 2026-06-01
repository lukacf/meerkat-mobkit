#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

command="${1:---smoke}"
if [[ $# -gt 0 ]]; then
  shift
fi

case "$command" in
  --smoke)
    npm run mdm:smoke
    ;;
  --browser-smoke)
    npm run mdm:browser-smoke
    ;;
  --run)
    npx tsx 004-mdm-console-pack/run.ts --demo-llm --wait "$@"
    ;;
  *)
    echo "Usage: $0 [--smoke|--browser-smoke|--run --targets <target-bindings.json>]" >&2
    exit 2
    ;;
esac
