#!/usr/bin/env bash
# Runs the Hurl end-to-end tests against the bootstrap example.
#
# Usage: tests/hurl/run.sh [hurl options...], e.g. `tests/hurl/run.sh --verbose`.
# Requires Hurl 7 (https://hurl.dev). Set PORT to change the server port (default 3999).
set -euo pipefail

cd "$(dirname "$0")/../.."

port="${PORT:-3999}"
base_url="http://127.0.0.1:$port"
token="hurl-token"

cargo build --example bootstrap
HOST=127.0.0.1 PORT="$port" API_TOKEN="$token" MAX_ATTACHMENT_BYTES=64 \
    "${CARGO_TARGET_DIR:-target}/debug/examples/bootstrap" >/dev/null &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT

for _ in $(seq 50); do
    curl -fs "$base_url/health" >/dev/null && break
    kill -0 "$server" 2>/dev/null || { echo "The server exited before it was ready" >&2; exit 1; }
    sleep 0.1
done

hurl --test \
    --variable "base_url=$base_url" \
    --variable "token=$token" \
    "$@" \
    tests/hurl/*.hurl
