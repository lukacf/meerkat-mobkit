#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

case "${1:---smoke}" in
  --smoke)
    npm run mdm:smoke
    ;;
  --browser-smoke)
    npm run mdm:browser-smoke
    ;;
  --auth-smoke)
    npm run mdm:auth-smoke
    ;;
  --docker-smoke)
    ./004-mdm-console-pack/scripts/docker-smoke.sh
    ;;
  --run)
    npx tsx 004-mdm-console-pack/run.ts --spawn-targets --demo-llm --wait
    ;;
  *)
    echo "Usage: $0 [--smoke|--auth-smoke|--browser-smoke|--docker-smoke|--run]" >&2
    exit 2
    ;;
esac
