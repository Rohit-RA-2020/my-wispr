#!/usr/bin/env bash
set -euo pipefail

TARGET_USER="${1:-${SUDO_USER:-}}"
if [[ -z "${TARGET_USER}" ]]; then
  echo "usage: sudo ./scripts/setup-uinput.sh <username>" >&2
  exit 1
fi

groupadd -f wisprinput
cat >/etc/udev/rules.d/85-wispr-uinput.rules <<'EOF'
KERNEL=="uinput", GROUP="wisprinput", MODE="0660"
EOF
usermod -aG wisprinput "${TARGET_USER}"
udevadm control --reload-rules
udevadm trigger --name-match=uinput

echo "Wispr uinput setup is complete. Log out and back in for ${TARGET_USER}."
