#!/usr/bin/env bash
# Evaluate Umi-OCR against the 3 standard test PNGs.
# Requires: start-umi-ocr.sh already running, curl, base64, jq
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ASSETS="${SCRIPT_DIR}/../../assets"
API="http://127.0.0.1:1224/api/ocr"

# Verify API is up
if ! curl -sf "${API}/get_options" >/dev/null 2>&1; then
    echo "ERROR: Umi-OCR API not reachable at ${API}" >&2
    echo "       Run ./start-umi-ocr.sh first." >&2
    exit 1
fi

TMPJSON="$(mktemp)"
trap 'rm -f "$TMPJSON"' EXIT

# Helper: build JSON body via temp file (images too large for inline args)
call_ocr() {
    local img_path="$1"
    local b64
    b64=$(base64 -w0 "$img_path")
    cat > "$TMPJSON" <<ENDJSON
{
    "base64": "${b64}",
    "options": {
        "ocr.language": "models/config_japan.txt",
        "data.format": "dict"
    }
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

echo "=== Umi-OCR Evaluation ==="
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

    # Parse response
    code=$(echo "$response" | jq -r '.code // "?"')
    time_s=$(echo "$response" | jq -r '.time // "?"')

    if [[ "$code" == "100" ]]; then
        # Extract all recognized text lines
        ocr_text=$(echo "$response" | jq -r '.data[]?.text // empty' | tr '\n' ' ' | sed 's/ *$//')
        scores=$(echo "$response" | jq -r '.data[]?.score // empty' | tr '\n' ',' | sed 's/,$//')

        echo "Got:      ${ocr_text}"
        echo "Scores:   ${scores}"
        echo "Time:     ${time_s}s (${elapsed_ms}ms wall)"

        # Simple match check (exact or contains)
        if [[ "$ocr_text" == *"$expected"* ]] || [[ "$expected" == *"$ocr_text"* ]]; then
            echo "Result:   PASS"
            PASS=$((PASS + 1))
        else
            echo "Result:   FAIL (text mismatch)"
            FAIL=$((FAIL + 1))
        fi
    elif [[ "$code" == "101" ]]; then
        echo "Got:      (no text detected)"
        echo "Time:     ${time_s}s (${elapsed_ms}ms wall)"
        echo "Result:   FAIL (nothing detected)"
        FAIL=$((FAIL + 1))
    else
        echo "Got:      ERROR code=${code}"
        echo "Response: $(echo "$response" | jq -c .)"
        echo "Result:   FAIL"
        FAIL=$((FAIL + 1))
    fi
    echo ""
done

echo "=== Summary: ${PASS} passed, ${FAIL} failed out of ${#TESTS[@]} ==="
