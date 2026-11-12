#!/usr/bin/env bash
set -euo pipefail

packages=(
  build-essential
  pkg-config
  libwebkit2gtk-4.1-dev
  libgtk-3-dev
  libayatana-appindicator3-dev
  librsvg2-dev
  libxdo-dev
  libdbus-1-dev
  libxcb1-dev
  libxcb-randr0-dev
  libxrandr-dev
  libpipewire-0.3-dev
  libclang-dev
  libgbm-dev
)
packages+=("$@")

apt_options=(
  -o Acquire::Retries=5
  -o Acquire::http::Timeout=30
  -o Acquire::https::Timeout=30
  -o DPkg::Lock::Timeout=120
)

for attempt in 1 2 3; do
  if sudo apt-get "${apt_options[@]}" update \
    && sudo DEBIAN_FRONTEND=noninteractive apt-get "${apt_options[@]}" install \
      -y --no-install-recommends "${packages[@]}"; then
    exit 0
  fi

  if (( attempt == 3 )); then
    exit 1
  fi
  echo "Linux dependency installation failed; retrying ($attempt/3)." >&2
  sleep $((attempt * 10))
done
