#!/usr/bin/env bash
# OCR smoke-test launcher.
#
# Builds lenzu/src/bin/ocr_test.rs (if not already up-to-date) and runs it.
# All arguments are passed through directly to the binary.
#
# Usage:
#   ./scripts/test-ocr.sh [options]
#
# Options (passed to ocr-test binary):
#   --ollama-model NAME      local ollama model (repeatable; default: gemma4:e2b)
#                            e.g. --ollama-model gemma4:e2b --ollama-model glm-ocr --ollama-model qwen2.5:1.5b
#   --remote-model NAME      OpenRouter model (default: google/gemini-2.0-flash-001)
#   --remote-key KEY         OpenRouter API key (default: $OPENROUTER_API_KEY)
#   --all-local              test full production chain (gemma4 → glm-ocr → qwen2.5vl); implies --skip-remote
#   --skip-ollama            skip local-ollama test
#   --skip-remote            skip remote-OpenRouter test
#   --timeout N              per-backend timeout in seconds (default: 60)
#   --no-timeout             wait indefinitely (measure CPU inference time)
#   --max-dim N              longest-edge pixel cap (default: 640)
#   --num-ctx N              ollama num_ctx override (default: 2048; 0 = disable)
#   --image PATH             test image (default: assets/Unit-test-sample-texts.png)
#   --expected PATH          expected JSON (default: assets/Unit-test-sample-texts.json)
#
# Exit codes:
#   0 — all checks passed
#   1 — one or more checks failed or a backend was unreachable

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="$REPO_ROOT/target/release/ocr-test"

# Build the binary (no-op if already up-to-date).
echo "==> Building ocr-test..."
cargo build -p lenzu --bin ocr-test --release --manifest-path "$REPO_ROOT/lenzu/Cargo.toml" 2>&1
echo ""

# Run — pass all arguments straight through.
exec "$BINARY" "$@"
