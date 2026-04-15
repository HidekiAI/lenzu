#!/usr/bin/env bash
# Evaluate PaddleOCR-VL-For-Manga (GGUF via llama-server) against 3 test PNGs.
# Requires: serve.sh running on port 9999, curl, base64, jq
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ASSETS="${SCRIPT_DIR}/../../assets"
API="http://127.0.0.1:9999/v1/chat/completions"

# Verify API is up
if ! curl -sf http://127.0.0.1:9999/health >/dev/null 2>&1; then
    echo "ERROR: llama-server not reachable at port 9999" >&2
    echo "       Run ./serve.sh first." >&2
    exit 1
fi

TMPJSON="$(mktemp)"
trap 'rm -f "$TMPJSON"' EXIT

# Helper: call llama-server with image
call_ocr() {
    local img_path="$1"
    local b64
    b64=$(base64 -w0 "$img_path")
    cat > "$TMPJSON" <<ENDJSON
{
    "messages": [
        {
            "role": "user",
            "content": [
                {"type": "text", "text": "<__media__>OCR:"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,${b64}"}}
            ]
        }
    ],
    "temperature": 0,
    "max_tokens": 256
}
ENDJSON
    curl -sf -X POST "${API}" \
        -H "Content-Type: application/json" \
        -d @"$TMPJSON"
}

# Test cases: file | expected_text | orientation
TESTS=(
    "Unit-test-yokogaki.png|データを正確に読み取る|horizontal"
    "Unit-test-tategaki.png|『言語モデルのテスト』|vertical"
    "Unit-test-tegaki.png|手書きの文字サンプル|horizontal (calligraphy)"
)

PASS=0
FAIL=0

echo "=== PaddleOCR-VL-For-Manga (GGUF) Evaluation ==="
echo ""

for entry in "${TESTS[@]}"; do
    IFS='|' read -r file expected orientation <<< "$entry"
    img_path="${ASSETS}/${file}"

    if [[ ! -f "$img_path" ]]; then
        echo "SKIP: ${file} — not found"
        continue
    fi

    echo "--- ${file} (${orientation}) ---"
    echo "Expected: ${expected}"

    t0=$(date +%s%N)
    response=$(call_ocr "$img_path" 2>&1) || {
        echo "ERROR: API call failed"
        echo "$response"
        FAIL=$((FAIL + 1))
        echo ""
        continue
    }
    t1=$(date +%s%N)
    elapsed_ms=$(( (t1 - t0) / 1000000 ))

    # Parse OpenAI-compatible response
    ocr_text=$(echo "$response" | jq -r '.choices[0].message.content // "ERROR"' | tr '\n' ' ' | sed 's/ *$//')
    tokens=$(echo "$response" | jq -r '.usage.completion_tokens // "?"')

    echo "Got:      ${ocr_text}"
    echo "Tokens:   ${tokens}"
    echo "Time:     ${elapsed_ms}ms"

    # Simple match check (exact or contains)
    if [[ "$ocr_text" == *"$expected"* ]] || [[ "$expected" == *"$ocr_text"* ]]; then
        echo "Result:   PASS"
        PASS=$((PASS + 1))
    else
        echo "Result:   FAIL (text mismatch)"
        FAIL=$((FAIL + 1))
    fi
    echo ""
done

echo "=== Summary: ${PASS} passed, ${FAIL} failed out of ${#TESTS[@]} ==="
