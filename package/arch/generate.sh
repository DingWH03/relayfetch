#!/usr/bin/env bash
set -e

VERSION="$1"
if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version>"
  exit 1
fi

# 源码包
mkdir -p dist-src
sed "s/@VERSION@/$VERSION/g" PKGBUILD.src.in > dist-src/PKGBUILD
cp relayfetch.install relayfetch.service config.toml files.toml dist-src/
(cd dist-src && makepkg --printsrcinfo > .SRCINFO)

# 二进制包
mkdir -p dist-bin
sed "s/@VERSION@/$VERSION/g" PKGBUILD.bin.in > dist-bin/PKGBUILD
cp relayfetch.install relayfetch.service config.toml files.toml dist-bin/
(cd dist-bin && makepkg --printsrcinfo > .SRCINFO)
