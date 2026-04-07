#!/usr/bin/env bash
set -euo pipefail

APP_DIR="${HOME}/Applications/Wispr.app"
CONFIG_DIR="${HOME}/.wispr"
SUPPORT_DIR="${HOME}/Library/Application Support/wispr"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

removed=0
skipped=0

step() { echo -e "\n${BOLD}$1${NC}"; }
ok()   { echo -e "  ${GREEN}✓${NC} $1"; removed=$((removed + 1)); }
skip() { echo -e "  ${YELLOW}–${NC} $1 (not found)"; skipped=$((skipped + 1)); }

echo -e "${BOLD}Wispr Mac — Complete Uninstaller${NC}"
echo "This will remove the app, config, cached data, keychain secrets,"
echo "and build artifacts from this machine."
echo ""

if [[ "${1:-}" != "--yes" ]]; then
    read -rp "Proceed? [y/N] " confirm
    if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
        echo "Aborted."
        exit 0
    fi
fi

# ── 1. Kill running processes ────────────────────────────────────────────────
step "Stopping processes…"

killed=false
for name in wisprd WisprMacApp; do
    if pkill -9 -f "$name" 2>/dev/null; then
        ok "Killed $name"
        killed=true
    fi
done
if ! $killed; then
    skip "No Wispr processes running"
fi
sleep 1

# ── 2. Remove app bundle ────────────────────────────────────────────────────
step "Removing app bundle…"

if [[ -d "$APP_DIR" ]]; then
    rm -rf "$APP_DIR"
    ok "Removed $APP_DIR"
else
    skip "$APP_DIR"
fi

# ── 3. Remove config & logs ─────────────────────────────────────────────────
step "Removing config & logs…"

if [[ -d "$CONFIG_DIR" ]]; then
    rm -rf "$CONFIG_DIR"
    ok "Removed $CONFIG_DIR"
else
    skip "$CONFIG_DIR"
fi

# ── 4. Remove Application Support (socket, runtime config) ──────────────────
step "Removing Application Support data…"

if [[ -d "$SUPPORT_DIR" ]]; then
    rm -rf "$SUPPORT_DIR"
    ok "Removed $SUPPORT_DIR"
else
    skip "$SUPPORT_DIR"
fi

# ── 5. Remove keychain entries ───────────────────────────────────────────────
step "Removing keychain entries…"

for service in io.wispr.deepgram io.wispr.intelligence io.wispr.generation; do
    if security find-generic-password -s "$service" &>/dev/null; then
        security delete-generic-password -s "$service" &>/dev/null
        ok "Deleted keychain: $service"
    else
        skip "Keychain: $service"
    fi
done

# ── 6. Remove build artifacts (only when run from the repo) ─────────────────
step "Removing build artifacts…"

if [[ -d "$ROOT_DIR/target" ]]; then
    size=$(du -sh "$ROOT_DIR/target" 2>/dev/null | cut -f1)
    rm -rf "$ROOT_DIR/target"
    ok "Removed target/ ($size)"
else
    skip "target/"
fi

if [[ -d "$ROOT_DIR/apps/WisprMac/.build" ]]; then
    size=$(du -sh "$ROOT_DIR/apps/WisprMac/.build" 2>/dev/null | cut -f1)
    rm -rf "$ROOT_DIR/apps/WisprMac/.build"
    ok "Removed apps/WisprMac/.build/ ($size)"
else
    skip "apps/WisprMac/.build/"
fi

# ── 7. Summary & manual steps ───────────────────────────────────────────────
echo ""
echo -e "${BOLD}Done.${NC}  ${GREEN}${removed} removed${NC}, ${YELLOW}${skipped} skipped${NC}."
echo ""
echo -e "${YELLOW}Manual steps remaining:${NC}"
echo "  1. System Settings → Privacy & Security → Accessibility"
echo "     Remove 'wisprd' and/or 'osascript' if listed."
echo "  2. System Settings → Privacy & Security → Microphone"
echo "     Remove 'Wispr' if listed."
echo ""
echo "Source code in ${ROOT_DIR} was kept intact."
echo "To reinstall:  bash scripts/install_wispr_mac_dev.sh"
