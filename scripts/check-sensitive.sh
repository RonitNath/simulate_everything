#!/usr/bin/env bash
# Pre-commit hook: detect sensitive patterns in staged files.
# Install: ln -sf ../../scripts/check-sensitive.sh .git/hooks/pre-commit
#
# Also usable standalone: ./scripts/check-sensitive.sh [file...]
# With no args, checks git staged files.

set -euo pipefail

RED='\033[0;31m'
NC='\033[0m'

if [ $# -gt 0 ]; then
    files=("$@")
else
    mapfile -t files < <(git diff --cached --name-only --diff-filter=ACM)
fi

if [ ${#files[@]} -eq 0 ]; then
    exit 0
fi

errors=0

check_pattern() {
    local pattern="$1"
    local description="$2"
    local exclude="${3:-}"

    for file in "${files[@]}"; do
        # Skip binary files, this script itself, and the .example service file
        [[ "$file" == *.png ]] && continue
        [[ "$file" == scripts/check-sensitive.sh ]] && continue
        [[ "$file" == simulate_everything.service.example ]] && continue
        [[ -n "$exclude" && "$file" == $exclude ]] && continue

        if git show :"$file" 2>/dev/null | grep -qP "$pattern"; then
            echo -e "${RED}BLOCKED${NC}: $description in $file"
            git show :"$file" | grep -nP "$pattern" | head -3
            echo ""
            errors=$((errors + 1))
        fi
    done
}

# Private LAN IPs (192.168.x.x, 10.x.x.x, 172.16-31.x.x)
check_pattern '192\.168\.\d+\.\d+' "Private IP address (192.168.x.x)"
check_pattern '\b10\.\d+\.\d+\.\d+\b' "Private IP address (10.x.x.x)" "*.md"
check_pattern '172\.(1[6-9]|2\d|3[01])\.\d+\.\d+' "Private IP address (172.16-31.x.x)"

# Absolute home directory paths
check_pattern '/home/\w+/' "Absolute home directory path"

# Hardcoded usernames in service files
check_pattern '^User=\w+' "Hardcoded username in service file" "*.example"

# Common secret patterns
check_pattern '(api[_-]?key|secret[_-]?key|password)\s*[:=]\s*["\x27][^"\x27]{8,}' "Possible hardcoded secret"

if [ $errors -gt 0 ]; then
    echo -e "${RED}Found $errors sensitive pattern(s). Fix before committing.${NC}"
    echo "If intentional, use: git commit --no-verify"
    exit 1
fi
