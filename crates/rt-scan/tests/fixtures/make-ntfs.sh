#!/bin/bash
# Builds ntfs-small.img.zlib: an 8 MB NTFS volume made by mkntfs, with files
# copied in by ntfscp, compressed with zlib. The MFT reader's tests
# (src/scan/ntfs.rs) read it.
#
# Needs the ntfs-3g tools; without root, extract them first:
#   mkdir -p ~/ntfs && cd ~/ntfs && apt-get download ntfs-3g libntfs-3g89t64
# Usage: make-ntfs.sh <output.zlib>
set -e
R="$HOME/ntfs/root"
for f in "$HOME"/ntfs/*.deb; do dpkg -x "$f" "$R"; done
export LD_LIBRARY_PATH="$R/usr/lib/x86_64-linux-gnu:$R/lib/x86_64-linux-gnu"
PATH="$R/usr/bin:$R/sbin:$R/usr/sbin:$R/bin:$PATH"
W=$(mktemp -d)
IMG="$W/vol.img"
truncate -s 8M "$IMG"
mkntfs -F -f -q -s 512 -c 4096 -L rsize-test "$IMG"
printf 'small file, kept in the MFT record\n' > "$W/small.txt"
yes 'big file line' | head -c 50000 > "$W/big.bin"
yes 'larger file line' | head -c 300000 > "$W/larger.dat"
printf 'x' > "$W/one.txt"
printf 'ação' > "$W/acentos.txt"
ntfscp -q "$IMG" "$W/small.txt" /small.txt
ntfscp -q "$IMG" "$W/big.bin" /big.bin
ntfscp -q "$IMG" "$W/larger.dat" /larger.dat
ntfscp -q "$IMG" "$W/acentos.txt" "/ação ü.txt"
ntfscp -q "$IMG" "$W/one.txt" '/$Extend/one.txt' || echo "no copy into \$Extend"
# A named stream on small.txt.
ntfscp -q -N extra "$IMG" "$W/big.bin" /small.txt || echo "no named stream"
ntfsls -l -a -s -i -p / "$IMG" || true
echo "--- \$Extend"
ntfsls -l -a -s -i -p '/$Extend' "$IMG" || true
python3 -c "import sys,zlib; open(sys.argv[2],'wb').write(zlib.compress(open(sys.argv[1],'rb').read(),9))" "$IMG" "$1"
ls -l "$1"
rm -rf "$W"
