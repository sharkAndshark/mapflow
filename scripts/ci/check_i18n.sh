#!/bin/bash
# i18n detection script
# 1. Detect hardcoded Chinese strings in JSX files
# 2. Validate t() keys exist in both en.json and zh.json

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FRONTEND_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)/frontend"
LOCALES_DIR="$FRONTEND_DIR/src/i18n/locales"
SRC_DIR="$FRONTEND_DIR/src"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ERRORS=0

# Files that are allowed to contain Chinese (language definitions, etc.)
HARDCODED_WHITELIST=(
    "src/LanguageSwitcher.jsx"
)

echo "=== i18n Detection ==="
echo ""

# =============================================================================
# Check 1: Hardcoded Chinese strings
# =============================================================================
echo "Check 1: Hardcoded Chinese strings..."

# Find JSX files excluding locales and node_modules
CHINESE_FILES=$(grep -rl --include="*.jsx" --include="*.js" \
    --exclude-dir="node_modules" \
    --exclude-dir="locales" \
    -P '[\p{Han}]' \
    "$SRC_DIR" 2>/dev/null || true)

VIOLATIONS=""

if [ -n "$CHINESE_FILES" ]; then
    for file in $CHINESE_FILES; do
        rel_path="${file#$FRONTEND_DIR/}"
        
        # Check if file is in whitelist
        IS_WHITELISTED=0
        for whitelisted in "${HARDCODED_WHITELIST[@]}"; do
            if [ "$rel_path" = "$whitelisted" ]; then
                IS_WHITELISTED=1
                break
            fi
        done
        
        if [ $IS_WHITELISTED -eq 0 ]; then
            # Filter out comments (lines starting with // or /* or containing only Chinese in comments)
            CHINESE_LINES=$(grep -n -P '[\p{Han}]' "$file" | grep -v '^\s*//' | grep -v '^\s*\*' | grep -v 'console\.' || true)
            if [ -n "$CHINESE_LINES" ]; then
                VIOLATIONS="$VIOLATIONS\n  $rel_path:\n"
                VIOLATIONS="$VIOLATIONS$(echo "$CHINESE_LINES" | head -5 | sed 's/^/    /')\n"
            fi
        fi
    done
    
    if [ -n "$VIOLATIONS" ]; then
        echo -e "${RED}ERROR: Found hardcoded Chinese strings:${NC}"
        echo -e "$VIOLATIONS"
        ERRORS=$((ERRORS + 1))
    else
        echo -e "${GREEN}PASS: No hardcoded Chinese strings found (whitelisted files excluded)${NC}"
    fi
else
    echo -e "${GREEN}PASS: No hardcoded Chinese strings found${NC}"
fi

echo ""

# =============================================================================
# Check 2: t() key completeness
# =============================================================================
echo "Check 2: t() key completeness..."

EN_JSON="$LOCALES_DIR/en.json"
ZH_JSON="$LOCALES_DIR/zh.json"

if [ ! -f "$EN_JSON" ] || [ ! -f "$ZH_JSON" ]; then
    echo -e "${RED}ERROR: Locale files not found${NC}"
    echo "  Expected: $EN_JSON"
    echo "  Expected: $ZH_JSON"
    ERRORS=$((ERRORS + 1))
else
    # Extract all t('xxx') keys from source files
    # Match patterns: t('key'), t("key")
    # Use word boundary to avoid matching getId('fid') as t('fid')
    # Exclude: CSS selectors (starting with .), paths (containing /), numbers
    USED_KEYS=$(grep -roh --include="*.jsx" --include="*.js" \
        --exclude-dir="node_modules" \
        --exclude-dir="locales" \
        -E "\bt\(['\"]([a-zA-Z][a-zA-Z0-9_.\-]*)['\"]\)" \
        "$SRC_DIR" 2>/dev/null | \
        sed -E "s/\bt\(['\"]([a-zA-Z][a-zA-Z0-9_.\-]*)['\"]\)/\1/" | \
        sort -u || true)

    if [ -z "$USED_KEYS" ]; then
        echo -e "${YELLOW}WARN: No t() calls found in source files${NC}"
    else
        MISSING_IN_EN=""
        MISSING_IN_ZH=""

        # Check each key against locale files
        while IFS= read -r key; do
            # Skip empty keys
            [ -z "$key" ] && continue

            # Check in en.json
            if ! node -e "
                const data = require('$EN_JSON');
                const parts = '$key'.split('.');
                let obj = data;
                for (const part of parts) {
                    if (obj && typeof obj === 'object' && part in obj) {
                        obj = obj[part];
                    } else {
                        process.exit(1);
                    }
                }
            " 2>/dev/null; then
                MISSING_IN_EN="$MISSING_IN_EN\n  $key"
            fi

            # Check in zh.json
            if ! node -e "
                const data = require('$ZH_JSON');
                const parts = '$key'.split('.');
                let obj = data;
                for (const part of parts) {
                    if (obj && typeof obj === 'object' && part in obj) {
                        obj = obj[part];
                    } else {
                        process.exit(1);
                    }
                }
            " 2>/dev/null; then
                MISSING_IN_ZH="$MISSING_IN_ZH\n  $key"
            fi
        done <<< "$USED_KEYS"

        if [ -n "$MISSING_IN_EN" ]; then
            echo -e "${RED}ERROR: Keys missing in en.json:${NC}"
            echo -e "$MISSING_IN_EN"
            ERRORS=$((ERRORS + 1))
        fi

        if [ -n "$MISSING_IN_ZH" ]; then
            echo -e "${RED}ERROR: Keys missing in zh.json:${NC}"
            echo -e "$MISSING_IN_ZH"
            ERRORS=$((ERRORS + 1))
        fi

        if [ -z "$MISSING_IN_EN" ] && [ -z "$MISSING_IN_ZH" ]; then
            echo -e "${GREEN}PASS: All t() keys exist in both locale files${NC}"
        fi
    fi
fi

echo ""
echo "=== Summary ==="

if [ $ERRORS -gt 0 ]; then
    echo -e "${RED}FAILED: $ERRORS error(s) found${NC}"
    exit 1
else
    echo -e "${GREEN}PASSED: All i18n checks passed${NC}"
    exit 0
fi
