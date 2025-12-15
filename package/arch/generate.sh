#!/usr/bin/env bash
set -e

VERSION="$1"
if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version>"
  exit 1
fi

ARCHS=("amd64" "aarch64" "armv7h" "riscv64")
declare -A ARCH_MAP
ARCH_MAP=( ["amd64"]="x86_64" ["aarch64"]="aarch64" ["armv7h"]="armv7" ["riscv64"]="riscv64" )

# -------------------------
# 生成源码包
# -------------------------
OUTDIR="dist-src/relayfetch"
rm -rf "$OUTDIR"
mkdir -p "$OUTDIR"

# 生成 PKGBUILD
sed "s/@VERSION@/$VERSION/g" PKGBUILD.src.in > "$OUTDIR/PKGBUILD"

# 拷贝附属文件
cp relayfetch.install \
   relayfetch.service \
   config.toml \
   files.toml \
   "$OUTDIR/"

# 生成 .SRCINFO（不构建）
(
  cd "$OUTDIR"
  makepkg --printsrcinfo > .SRCINFO
)

# -------------------------
# 生成二进制包
# -------------------------
for arch in "${ARCHS[@]}"; do
  OUTDIR="dist-bin/$arch"
  RUST_ARCH=${ARCH_MAP[$arch]}
  mkdir -p "$OUTDIR"

  sed -e "s/@VERSION@/$VERSION/g" -e "s/@ARCH@/$arch/g" -e "s/@RUST_ARCH@/$RUST_ARCH/g" PKGBUILD.bin.in > "$OUTDIR/PKGBUILD"
  cp relayfetch.install relayfetch.service config.toml files.toml "$OUTDIR/"
  (cd "$OUTDIR" && makepkg --printsrcinfo > .SRCINFO)
done

echo "==> 生成完成，目录结构："
tree dist-src dist-bin
