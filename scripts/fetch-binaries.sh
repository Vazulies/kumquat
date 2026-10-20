#!/usr/bin/env bash
# Downloads the conversion programs Kumquat bundles into resources/binaries/.
#
#   scripts/fetch-binaries.sh                     # ffmpeg, ImageMagick, pandoc, 7-Zip, PDFium
#   scripts/fetch-binaries.sh --with-libreoffice  # also LibreOffice (~350 MB)
#   scripts/fetch-binaries.sh --only pdfium,pandoc
#
# Everything comes from the projects' own release pages. Linux x86_64 only.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/resources/binaries"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

WITH_LO=0
ONLY=""
while [ $# -gt 0 ]; do
  case "$1" in
    --with-libreoffice) WITH_LO=1 ;;
    --only) ONLY="$2"; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done

if [ "$(uname -m)" != "x86_64" ]; then
  echo "Only x86_64 downloads are set up in this script." >&2
  exit 1
fi

wanted() { [ -z "$ONLY" ] || [[ ",$ONLY," == *",$1,"* ]]; }

say() { printf '\n\033[1;33m==>\033[0m %s\n' "$*"; }

fetch() { # url file
  curl -fL --retry 3 --progress-bar -o "$2" "$1"
}

# Asset URL from a GitHub repository's latest release, matching a regex.
github_asset() { # owner/repo regex
  curl -fsSL "https://api.github.com/repos/$1/releases/latest" |
    grep -oE '"browser_download_url": *"[^"]+"' |
    sed -E 's/.*"(https[^"]+)"/\1/' |
    grep -E "$2" | head -n1
}

mkdir -p "$DEST/lib"

if wanted ffmpeg; then
  say "ffmpeg + ffprobe (BtbN static GPL build)"
  url="$(curl -fsSL https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/tags/latest |
    grep -oE '"browser_download_url": *"[^"]+"' | sed -E 's/.*"(https[^"]+)"/\1/' |
    grep -E 'ffmpeg-n[0-9.]+-latest-linux64-gpl-[0-9.]+\.tar\.xz$' | sort -V | tail -n1)"
  fetch "$url" "$WORK/ffmpeg.tar.xz"
  tar -xf "$WORK/ffmpeg.tar.xz" -C "$WORK"
  install -m755 "$WORK"/ffmpeg-*/bin/ffmpeg "$WORK"/ffmpeg-*/bin/ffprobe "$DEST/"
fi

if wanted magick; then
  say "ImageMagick (official AppImage, unpacked so it runs without FUSE)"
  url="$(github_asset ImageMagick/ImageMagick 'gcc-x86_64\.AppImage$')"
  fetch "$url" "$WORK/magick.AppImage"
  chmod +x "$WORK/magick.AppImage"
  (cd "$WORK" && ./magick.AppImage --appimage-extract >/dev/null)
  rm -rf "$DEST/imagemagick"
  mv "$WORK/squashfs-root" "$DEST/imagemagick"
  cat > "$DEST/magick" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/imagemagick/AppRun" "$@"
EOF
  chmod +x "$DEST/magick"
fi

if wanted pandoc; then
  say "pandoc (static build)"
  url="$(github_asset jgm/pandoc 'linux-amd64\.tar\.gz$')"
  fetch "$url" "$WORK/pandoc.tar.gz"
  tar -xzf "$WORK/pandoc.tar.gz" -C "$WORK"
  install -m755 "$WORK"/pandoc-*/bin/pandoc "$DEST/pandoc"
fi

if wanted 7zz; then
  say "7-Zip (for RAR and 7z)"
  url="$(github_asset ip7z/7zip 'linux-x64\.tar\.xz$')"
  mkdir -p "$WORK/7z"
  fetch "$url" "$WORK/7z.tar.xz"
  tar -xf "$WORK/7z.tar.xz" -C "$WORK/7z"
  install -m755 "$WORK/7z/7zz" "$DEST/7zz"
fi

if wanted pdfium; then
  say "PDFium (bblanchon/pdfium-binaries)"
  url="$(github_asset bblanchon/pdfium-binaries 'pdfium-linux-x64\.tgz$')"
  mkdir -p "$WORK/pdfium"
  fetch "$url" "$WORK/pdfium.tgz"
  tar -xzf "$WORK/pdfium.tgz" -C "$WORK/pdfium"
  install -m644 "$WORK/pdfium/lib/libpdfium.so" "$DEST/lib/libpdfium.so"
fi

if [ "$WITH_LO" = 1 ] && wanted soffice; then
  say "LibreOffice (official AppImage)"
  page="$(curl -fsSL https://appimages.libreitalia.org/)"
  file="$(echo "$page" | grep -oE 'LibreOffice-still\.basic-x86_64\.AppImage' | head -n1)"
  fetch "https://appimages.libreitalia.org/${file:-LibreOffice-still.basic-x86_64.AppImage}" "$WORK/lo.AppImage"
  chmod +x "$WORK/lo.AppImage"
  (cd "$WORK" && ./lo.AppImage --appimage-extract >/dev/null)
  rm -rf "$DEST/libreoffice"
  mv "$WORK/squashfs-root" "$DEST/libreoffice"
  cat > "$DEST/soffice" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$(ls -d "$HERE"/libreoffice/opt/libreoffice*/program | head -n1)/soffice" "$@"
EOF
  chmod +x "$DEST/soffice"
fi

say "Done. In resources/binaries:"
du -sh "$DEST"/* 2>/dev/null | sed "s|$DEST/||"
