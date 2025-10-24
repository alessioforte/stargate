#!/bin/bash

# Load Test Runner
# Usage: ./run.sh [environment]

set -e

ENVIRONMENT=${1:-local}
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_URL=""

case $ENVIRONMENT in
    "local")
        BASE_URL="https://127.0.0.1:5050"
        ;;
    "staging")
        BASE_URL="https://staging-api.example.com"
        ;;
    "production")
        echo "WARNING: Running load test against production!"
        BASE_URL="https://api.example.com"
        ;;
    *)
        echo "Unknown environment: $ENVIRONMENT"
        exit 1
        ;;
esac

echo "Running load tests against: $BASE_URL"
echo "Script directory: $SCRIPT_DIR"

# Simple test
echo -e "\n=== Running Simple Test ==="
wrk -t4 -c100 -d10s "$BASE_URL/api/service3/v1/lb"

# Multiple users test
echo -e "\n=== Running Multiple Users Test ==="
wrk -t8 -c400 -d30s -s "$SCRIPT_DIR/scripts/multi-users.lua" "$BASE_URL/api/service3/v1/lb"

echo -e "\n=== All tests completed ==="
