#!/usr/bin/env sh
# Copyright (C) 2026 Johannes Asal
# SPDX-License-Identifier: AGPL-3.0-or-later
# See LICENSE in the project root for the full license and warranty disclaimer.

# Baut Sorobo (Frontend + Backend) und legt die fertigen Programme unter dist/ ab:
#   dist/sorobo          Desktop-Anwendung (eigenes Fenster)
#   dist/sorobo-server   Server ohne Fenster (Browser / Entwicklung)
#
# Linux: die Desktop-Anwendung benötigt webkit2gtk-4.1 und libgtk-3 (Entwicklungspakete).
# Nur den Server bauen:  SOROBO_SERVER_ONLY=1 ./build.sh
set -e
cd "$(dirname "$0")"
version=$(node scripts/version.mjs)

if [ "${SOROBO_FRONTEND_READY:-}" = "1" ]; then
  if [ ! -f frontend/dist/index.html ]; then
    echo "SOROBO_FRONTEND_READY requires an already built frontend/dist/index.html." >&2
    exit 1
  fi
  echo "[1-2/3] Vorbereitetes Frontend verwenden …"
else
  echo "[1/3] Frontend-Abhängigkeiten …"
  npm --prefix frontend ci --no-audit --no-fund
  echo "[2/3] Frontend bauen …"
  npm --prefix frontend run build
fi

echo "[3/3] Backend bauen (Release) …"
mkdir -p dist
if [ -n "$SOROBO_SERVER_ONLY" ]; then
  cargo build --locked --release --manifest-path backend/Cargo.toml --no-default-features --bin sorobo-server
  files="sorobo-server LICENSE.txt THIRD-PARTY-LICENSES.txt"
  variant="-server"
else
  cargo build --locked --release --manifest-path backend/Cargo.toml --bins
  cp backend/target/release/sorobo-desktop dist/sorobo
  files="sorobo sorobo-server LICENSE.txt THIRD-PARTY-LICENSES.txt"
  variant=""
fi
cp backend/target/release/sorobo-server dist/sorobo-server
cp LICENSE dist/LICENSE.txt
node scripts/third-party-licenses.mjs dist/THIRD-PARTY-LICENSES.txt
mkdir -p dist/packages
platform=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
# Only ship explicitly named files, never dist/data or old packages.
tar -czf "dist/packages/Sorobo-$version-$platform-$arch$variant.tar.gz" -C dist $files
echo
echo "Fertig: dist/"
