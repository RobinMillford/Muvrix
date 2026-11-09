#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

echo "=== [Muvrix Network Integration] Running Real-Server Integration Test Suite ==="

# Ensure services are up
"${SCRIPT_DIR}/setup.sh"

export LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-${HOME}/.local/lib64:/usr/local/lib64:/usr/local/lib}"
export MUVRIX_TEST_NATIVE_KEYRING="${MUVRIX_TEST_NATIVE_KEYRING:-0}"

cd "${BASE_DIR}"
echo "1. Running core real server integration tests..."
cargo test --package muvrix-core --test real_server_integration_test -- --nocapture

if [ "${MUVRIX_TEST_NATIVE_KEYRING}" = "1" ]; then
    echo "2. Running core secret service tests (live OS keyring opted-in)..."
else
    echo "2. Running core secret service tests (live OS keyring skipped by default; set MUVRIX_TEST_NATIVE_KEYRING=1 to opt in)..."
fi
cargo test --package muvrix-core --test secret_service_test -- --nocapture

echo "3. Running real playback integration tests in libmpv..."
cargo test --package muvrix-player --test real_playback_integration_test -- --nocapture

echo "4. Running SMB performance benchmark against live Samba..."
cargo test --package muvrix-player --test smb_performance_benchmark_test -- --nocapture

echo "5. Running network interruption and recovery tests..."
cargo test --package muvrix-player --test interruption_recovery_test -- --nocapture

echo "6. Running remote stream memory profiling..."
cargo test --package muvrix-player --test stream_memory_profiling_test -- --nocapture

echo "=== Real-Server Integration Tests Completed Successfully ==="
