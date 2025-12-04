#!/usr/bin/env bash
# Pre-commit check script for LogCrab
# Ensures code quality before committing changes

set -e  # Exit on first error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_step() {
    echo -e "${BLUE}==>${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

# Track overall success
FAILED=0

echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  LogCrab Pre-Commit Quality Checks    ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════╝${NC}"
echo ""

# 1. Check for uncommitted changes (optional warning)
print_step "Checking git status..."
if [[ -n $(git status --porcelain) ]]; then
    print_warning "You have uncommitted changes"
else
    print_success "Working directory is clean"
fi
echo ""

# 2. Format check with rustfmt
print_step "Checking code formatting with rustfmt..."
if cargo fmt -- --check > /dev/null 2>&1; then
    print_success "Code is properly formatted"
else
    print_error "Code formatting issues found"
    echo "  Run: cargo fmt"
    FAILED=1
fi
echo ""

# 3. Build check
print_step "Building project (debug)..."
if cargo build --quiet; then
    print_success "Build successful (debug)"
else
    print_error "Build failed"
    FAILED=1
fi
echo ""

# 4. Build check (release)
print_step "Building project (release)..."
if cargo build --release --quiet; then
    print_success "Build successful (release)"
else
    print_error "Release build failed"
    FAILED=1
fi
echo ""

# # 5. Run tests
# print_step "Running tests..."
# if cargo test --quiet; then
    # print_success "All tests passed"
# else
    # print_error "Tests failed"
    # FAILED=1
# fi
# echo ""

# 6. Clippy - standard lints
print_step "Running clippy (-D warnings)..."
if cargo clippy --all-targets --quiet -- -D warnings > /dev/null 2>&1; then
    print_success "Clippy checks passed (standard)"
else
    print_error "Clippy found issues (standard lints)"
    FAILED=1
fi
echo ""

# 6b. Clippy - standard lints
print_step "Running clippy (standard lints)..."
if cargo clippy --all-targets --quiet > /dev/null 2>&1; then
    print_success "Clippy checks passed (standard)"
else
    print_error "Clippy found issues (standard lints)"
    FAILED=1
fi
echo ""

# 7. Clippy - pedantic (warnings only for most items)
print_step "Running clippy (pedantic lints)..."
# Allow the most exotic/noisy pedantic lints
PEDANTIC_ALLOWS=(
"-Aclippy::cast_precision_loss"
"-Aclippy::cast_possible_truncation"
"-Aclippy::cast_sign_loss"
"-Aclippy::cast_possible_wrap"
"-Aclippy::match_same_arms"
)

if cargo clippy --all-targets --quiet -- -D clippy::pedantic "${PEDANTIC_ALLOWS[@]}" > /dev/null 2>&1; then
    print_success "Clippy checks passed (pedantic)"
else
    print_warning "Clippy found pedantic issues (review recommended)"
    # Don't fail on pedantic warnings
fi
echo ""

# 8. Check for common issues
print_step "Checking for common code issues..."

# Check for TODO/FIXME comments
TODO_COUNT=$(grep -r "TODO\|FIXME" src/ --include="*.rs" | wc -l || true)
if [ "$TODO_COUNT" -gt 0 ]; then
    print_warning "Found $TODO_COUNT TODO/FIXME comments"
else
    print_success "No TODO/FIXME comments"
fi

# Check for unwrap() calls (potential panics)
UNWRAP_COUNT=$(grep -r "\.unwrap()" src/ --include="*.rs" | grep -v "test\|#\[cfg(test)\]" | wc -l || true)
if [ "$UNWRAP_COUNT" -gt 0 ]; then
    print_warning "Found $UNWRAP_COUNT .unwrap() calls (consider using proper error handling)"
else
    print_success "No .unwrap() calls found"
fi

# Check for expect() calls
EXPECT_COUNT=$(grep -r "\.expect(" src/ --include="*.rs" | grep -v "test\|#\[cfg(test)\]" | wc -l || true)
if [ "$EXPECT_COUNT" -gt 0 ]; then
    print_warning "Found $EXPECT_COUNT .expect() calls"
else
    print_success "No .expect() calls found"
fi

# Check for println!/dbg! (should use proper logging)
DEBUG_COUNT=$(grep -r "println!\|dbg!" src/ --include="*.rs" | grep -v "test\|#\[cfg(test)\]" | wc -l || true)
if [ "$DEBUG_COUNT" -gt 0 ]; then
    print_warning "Found $DEBUG_COUNT println!/dbg! statements (consider using log macros)"
else
    print_success "No debug print statements"
fi
echo ""

# 9. Check dependencies for known vulnerabilities
print_step "Checking for dependency vulnerabilities..."
if command -v cargo-audit &> /dev/null; then
    if cargo audit --quiet; then
        print_success "No known vulnerabilities in dependencies"
    else
        print_warning "Vulnerabilities found in dependencies (review cargo audit output)"
    fi
else
    print_warning "cargo-audit not installed (install with: cargo install cargo-audit)"
fi
echo ""

# 10. Check for unused dependencies
print_step "Checking for unused dependencies..."
if command -v cargo-udeps &> /dev/null; then
    if cargo +nightly udeps --quiet 2>/dev/null; then
        print_success "No unused dependencies"
    else
        print_warning "Possible unused dependencies found"
    fi
else
    print_warning "cargo-udeps not installed (install with: cargo install cargo-udeps --locked)"
fi
echo ""

# 11. Generate documentation
print_step "Checking documentation generation..."
if cargo doc --no-deps --quiet; then
    print_success "Documentation builds successfully"
else
    print_error "Documentation generation failed"
    FAILED=1
fi
echo ""

# Summary
echo -e "${BLUE}════════════════════════════════════════${NC}"
if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All critical checks passed!${NC}"
    echo ""
    echo -e "  ${GREEN}Ready to commit!${NC} 🚀"
    exit 0
else
    echo -e "${RED}✗ Some checks failed${NC}"
    echo ""
    echo -e "  ${RED}Please fix the issues before committing${NC}"
    exit 1
fi
