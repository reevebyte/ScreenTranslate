#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/release/screentranslate"
APP_ID="io.github.reevebyte.screentranslate.v2"

if [[ ! -x "$BIN" ]]; then
  echo "Release binary not found at: $BIN" >&2
  echo "Build it first: pnpm --filter @screentranslate/desktop tauri build --no-bundle" >&2
  exit 1
fi

BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
ICON_BASE="${HOME}/.local/share/icons/hicolor"

mkdir -p "$BIN_DIR" "$APP_DIR" \
  "$ICON_BASE/scalable/apps" \
  "$ICON_BASE/128x128/apps" \
  "$ICON_BASE/32x32/apps"

install -m755 "$BIN" "$BIN_DIR/screentranslate"
install -m644 "$ROOT/apps/desktop/src-tauri/icons/app-icon.svg" "$ICON_BASE/scalable/apps/${APP_ID}.svg"
install -m644 "$ROOT/apps/desktop/src-tauri/icons/128x128.png" "$ICON_BASE/128x128/apps/${APP_ID}.png"
install -m644 "$ROOT/apps/desktop/src-tauri/icons/32x32.png" "$ICON_BASE/32x32/apps/${APP_ID}.png"

sed "s|^Exec=screentranslate|Exec=${BIN_DIR}/screentranslate|g" \
  "$ROOT/scripts/linux/${APP_ID}.desktop" > "$APP_DIR/${APP_ID}.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_DIR" || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$ICON_BASE" >/dev/null 2>&1 || true
fi

cat <<EOF
Installed ScreenTranslate to:
  Binary:  ${BIN_DIR}/screentranslate
  Desktop: ${APP_DIR}/${APP_ID}.desktop

Recommended Hyprland keybindings (~/.config/hypr/bindings.conf or hyprland.conf):
  bind = CTRL ALT, Q, exec, ${BIN_DIR}/screentranslate --capture
  bind = CTRL ALT, W, exec, ${BIN_DIR}/screentranslate --toggle-result
  bind = CTRL ALT, SPACE, exec, ${BIN_DIR}/screentranslate --quick
  bind = CTRL ALT, K, exec, ${BIN_DIR}/screentranslate --search
EOF
