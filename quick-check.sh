#!/usr/bin/env bash
# Quick pre-commit check script for LogCrab
# Runs essential checks quickly

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

print_step() { echo -e "${BLUE}==>${NC} $1"; }
print_success() { echo -e "${GREEN}✓${NC} $1"; }
print_error() { echo -e "${RED}✗${NC} $1"; }

FAILED=0

echo -e "${BLUE}Quick Pre-Commit Checks${NC}"
echo ""

# Format check
print_step "Format check..."
if cargo fmt -- --check --quiet > /dev/null 2>&1; then
    print_success "Formatted"
else
    print_error "Run: cargo fmt"
    FAILED=1
fi

# Build
print_step "Building..."
if cargo build 2>&1 | tail -1 | grep -q "Finished"; then
    print_success "Built"
else
    print_error "Build failed"
    FAILED=1
fi

# Clippy
print_step "Clippy..."
if cargo clippy --all-targets --quiet 2>&1 | grep -v "warning:" | tail -1 | grep -q "Finished"; then
    print_success "Clippy clean"
else
    print_error "Clippy issues"
    FAILED=1
fi

echo ""
if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ Quick checks passed!${NC} 🚀"
    exit 0
else
    echo -e "${RED}✗ Issues found${NC}"
    exit 1
fi
