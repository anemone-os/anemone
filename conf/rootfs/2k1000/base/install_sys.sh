#!/home/bin/busybox sh

echo Installing Anemone system overlay...
set -eu

BUSYBOX=/home/bin/busybox
ROOT=${1:-/}
ROOT=${ROOT%/}
INSTALL_MARKER="$ROOT/etc/.anemone-installed"
REINSTALL_MARKER=/home/.anemone_need_reinstall
INSTALL_VERSION=3

if [ ! -f "$REINSTALL_MARKER" ] \
    && [ -f "$INSTALL_MARKER" ] \
    && [ "$("$BUSYBOX" cat "$INSTALL_MARKER")" = "$INSTALL_VERSION" ]; then
    echo "Anemone system overlay is already installed."
    exit 0
fi

"$BUSYBOX" mkdir -p "$ROOT/etc" "$ROOT/root" "$ROOT/sbin"

echo "Copying configuration files..."

"$BUSYBOX" cp -Rf /home/etc/. "$ROOT/etc/"
"$BUSYBOX" cp -Rf /home/root/. "$ROOT/root/"
"$BUSYBOX" ln -sf /home/sbin/shutdown "$ROOT/sbin/poweroff"
echo "$INSTALL_VERSION" > "$INSTALL_MARKER"
"$BUSYBOX" rm -f "$REINSTALL_MARKER"

echo "Anemone system overlay is ready."
