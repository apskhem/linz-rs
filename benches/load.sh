#!/usr/bin/env bash
# Load-tests the `bench_server` example over HTTP and prints requests per second and latency
# percentiles per scenario. `plain/*` scenarios are the same endpoints written with plain axum.
#
# Usage: benches/load.sh [scenario filter], e.g. `benches/load.sh post`.
# Requires oha (https://github.com/hatoo/oha) and jq. Set DURATION (default 10s), CONNECTIONS
# (default 64), PORT (default 3998) and WORKERS (the server's threads, default 2) to change
# the run.
#
# oha runs on the same machine as the server and competes with it for CPU. With the server on
# a few threads it is the bottleneck, so the scenarios can be compared with each other; with
# WORKERS set to the number of cores, oha usually is, and the scenarios all look alike.
set -euo pipefail

cd "$(dirname "$0")/.."

for tool in oha jq curl; do
    command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done

filter="${1:-}"
duration="${DURATION:-10s}"
connections="${CONNECTIONS:-64}"
port="${PORT:-3998}"
workers="${WORKERS:-2}"
base_url="http://127.0.0.1:$port"

cargo build --release --example bench_server
PORT="$port" WORKERS="$workers" "${CARGO_TARGET_DIR:-target}/release/examples/bench_server" >/dev/null &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT

for _ in $(seq 50); do
    curl -fs "$base_url/openapi.json" >/dev/null && break
    kill -0 "$server" 2>/dev/null || { echo "The server exited before it was ready" >&2; exit 1; }
    sleep 0.1
done

new_user='{"username":"alice","age":30,"email":"alice@example.com"}'
invalid_user='{"username":"al","age":12,"email":"not-an-email"}'
failed=0

echo "duration: $duration, connections: $connections, server threads: $workers"
printf '\n%-24s %12s %10s %10s %10s  %s\n' scenario req/s p50_ms p99_ms p99.9_ms statuses

# run <name> <expected status> <path> [oha options...]
run() {
    local name="$1" expected="$2" path="$3"
    shift 3
    [[ -z "$filter" || "$name" == *"$filter"* ]] || return 0

    local result
    result=$(oha --no-tui --output-format json -z "$duration" -c "$connections" "$@" "$base_url$path")

    local statuses
    statuses=$(jq -r '.statusCodeDistribution | to_entries | map("\(.key)x\(.value)") | join(",")' <<<"$result")
    jq -r --arg name "$name" '[
        $name,
        (.summary.requestsPerSec | floor),
        (.latencyPercentiles.p50 * 1000 * 1000 | round / 1000),
        (.latencyPercentiles.p99 * 1000 * 1000 | round / 1000),
        (.latencyPercentiles["p99.9"] * 1000 * 1000 | round / 1000)
    ] | @tsv' <<<"$result" | {
        IFS=$'\t' read -r n rps p50 p99 p999
        printf '%-24s %12s %10s %10s %10s  %s\n' "$n" "$rps" "$p50" "$p99" "$p999" "$statuses"
    }

    if [[ "$(jq -r '.statusCodeDistribution | keys | join(",")' <<<"$result")" != "$expected" ]]; then
        echo "  expected only $expected responses" >&2
        failed=1
    fi
}

json=(-m POST -T application/json)

run plain/get_path_param 200 /plain/users/42
run get_path_param 200 /users/42
run plain/post_json 201 /plain/users "${json[@]}" -d "$new_user"
run post_json 201 /users "${json[@]}" -d "$new_user"
run post_json_invalid 400 /users "${json[@]}" -d "$invalid_user"
run get_query_list_10 200 '/users?limit=10&offset=0'
run get_query_list_100 200 '/users?limit=100&offset=0'
run put_urlencoded 200 /users/1 -m PUT -T application/x-www-form-urlencoded \
    -d 'username=alice&age=30&email=alice%40example.com'
run get_auth_cookie_header 200 /me -H 'Authorization: Bearer secret' -H 'Cookie: session=abc' \
    -H 'X-Client: load'
run not_found 404 /nowhere

exit "$failed"
