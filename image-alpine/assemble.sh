#!/bin/sh
set -eu
export SOURCE_DATE_EPOCH=1700000000
VER=$(cat pinned/alpine-rpi.version)
TARBALL=work/alpine-rpi-${VER}-aarch64.tar.gz
IMG=out/sosguide-${VER}.img
SERIAL=deadbeef
UUID=1b9d6bc5-c200-4a00-9f00-0123456789ab
P1_SECT=1048576          # 512 MiB
P2_SECT=2097152          # 1024 MiB
P1_START=2048
P2_START=$((P1_START + P1_SECT))
TOTAL=$((P2_START + P2_SECT))

echo ">> outils"
apk add --no-cache mtools dosfstools e2fsprogs util-linux tar gzip >/dev/null

echo ">> synchro des assets web (source de vérité unique = ../sosguide/web)"
# Le portail web servi par l'image DOIT être identique à celui du dépôt de dev :
# sans cette synchro, overlay/.../web est un snapshot figé qui dérive en silence
# (correctifs appliqués une seule fois, contenu vital divergent). On mirroir donc
# ../sosguide/web -> overlay, en PRÉSERVANT data/config.json (graine propre à
# l'image, jamais écrasée par un fichier de test du dev).
WEB_SRC=../sosguide/web
WEB_DST=overlay/usr/local/share/sosguide/web
if [ -d "$WEB_SRC" ]; then
    SEED=$(mktemp)
    [ -f "$WEB_DST/data/config.json" ] && cp "$WEB_DST/data/config.json" "$SEED"
    rm -rf "$WEB_DST"
    mkdir -p "$WEB_DST"
    cp -R "$WEB_SRC/." "$WEB_DST/"
    # audit.html n'existe plus (fusionnée dans /admin) : ne jamais l'embarquer.
    rm -f "$WEB_DST/audit.html"
    [ -s "$SEED" ] && mkdir -p "$WEB_DST/data" && cp "$SEED" "$WEB_DST/data/config.json"
    rm -f "$SEED"
    echo "   web synchronisé (config.json d'image préservée)."
else
    echo "   ⚠️  $WEB_SRC introuvable — overlay web laissé tel quel."
fi

echo ">> apkovl déterministe"
# Le vérificateur/updateur OTA doit être présent dans l'apkovl (sinon l'OTA
# reste inerte : la borne démarre sur le binaire d'usine, sans MAJ possible).
[ -x overlay/usr/local/bin/sos-cli ] || \
    echo "   ⚠️  overlay/usr/local/bin/sos-cli absent — OTA désactivée (binaire d'usine seul)."
tar --sort=name --numeric-owner --owner=0 --group=0 \
    --mtime=@${SOURCE_DATE_EPOCH} --format=ustar -C overlay -cf work/apkovl.tar .
gzip -n -9 -c work/apkovl.tar > work/sos-guide.apkovl.tar.gz
rm -f work/apkovl.tar

echo ">> racine FAT (SOSBOOT)"
rm -rf work/p1root && mkdir -p work/p1root
tar -xzf "$TARBALL" -C work/p1root
cat boot/config.txt.append >> work/p1root/config.txt
cp work/sos-guide.apkovl.tar.gz work/p1root/
# configuration du point d'accès WiFi éditable par l'utilisateur (racine FAT,
# sans rebuild) : SSID / COUNTRY / CHANNEL de l'AP ouvert diffusé par la borne
[ -f boot/ap.conf ] && cp boot/ap.conf work/p1root/ap.conf
# configuration de mise à jour OTA éditable par l'utilisateur (racine FAT) ;
# désactivée par défaut (ENABLED=0) tant qu'aucune URL/clé n'est fournie
[ -f boot/update.conf ] && cp boot/update.conf work/p1root/update.conf
# paquets WiFi (wpa_supplicant + deps) installés hors-ligne au boot
if [ -d boot-extra/aarch64 ]; then
    mkdir -p work/p1root/extra
    cp boot-extra/aarch64/*.apk work/p1root/extra/
fi
# DEBUG : console série + retrait de "quiet" (messages noyau visibles sur HDMI/série)
sed -i 's/ quiet//g' work/p1root/cmdline.txt
grep -q "console=ttyAMA0" work/p1root/cmdline.txt || \
    sed -i 's/$/ console=ttyAMA0,115200/' work/p1root/cmdline.txt
# horodatage figé (déterminisme FAT)
find work/p1root -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

echo ">> image partition 1 (FAT32 — mkfs.vfat, compatible bootloader Pi)"
truncate -s $((P1_SECT*512)) work/p1.img
# mkfs.vfat (dosfstools) produit un FAT32 standard (backup boot sector + FSInfo)
# que le bootloader EEPROM du Pi 4 lit, contrairement à mformat (mtools).
mkfs.vfat -F 32 -n SOSBOOT -i "$SERIAL" work/p1.img >/dev/null
# population récursive, dotfiles inclus (.alpine-release indispensable)
( cd work/p1root && for e in * .[!.]*; do [ -e "$e" ] && mcopy -i ../p1.img -s -Q -m "$e" :: ; done )

echo ">> image partition 2 (ext4 SOSDATA)"
truncate -s $((P2_SECT*512)) work/p2.img
mke2fs -F -q -t ext4 -L SOSDATA -U "$UUID" \
  -E hash_seed=$UUID,lazy_itable_init=0,lazy_journal_init=0 \
  -d data-skel work/p2.img >/dev/null 2>&1

echo ">> table MBR + concat"
truncate -s $((TOTAL*512)) "$IMG"
cat > work/mbr.sfdisk <<EOF
label: dos
label-id: 0x53534700
unit: sectors
start=$P1_START, size=$P1_SECT, type=c, bootable
start=$P2_START, size=$P2_SECT, type=83
EOF
sfdisk --no-reread --no-tell-kernel "$IMG" < work/mbr.sfdisk >/dev/null
dd if=work/p1.img of="$IMG" bs=512 seek=$P1_START conv=notrunc status=none
dd if=work/p2.img of="$IMG" bs=512 seek=$P2_START conv=notrunc status=none

echo ">> empreinte"
sha256sum "$IMG" | tee "$IMG.sha256"
ls -lh "$IMG"
