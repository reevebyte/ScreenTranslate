#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
pnpm install --frozen-lockfile
pnpm types:check
pnpm check
pnpm test
cargo test --workspace --locked
pnpm --filter @screentranslate/desktop tauri build "$@"
node scripts/measure-v2.mjs
