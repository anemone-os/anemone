#!/home/bin/busybox sh

echo Installing Anemone toolchain...
set -eu

BUSYBOX=/home/bin/busybox
# board-init invokes this after chrooting to /linux, so the default root `/`
# names the Linux root (the outer mount path is /linux).
ROOT=${1:-/}
ROOT=${ROOT%/}
INSTALL_MARKER="$ROOT/etc/.anemone-installed"
REINSTALL_MARKER=/home/.anemone_need_reinstall
INSTALL_VERSION=2

if [ ! -f "$REINSTALL_MARKER" ] \
    && [ -f "$INSTALL_MARKER" ] \
    && [ "$("$BUSYBOX" cat "$INSTALL_MARKER")" = "$INSTALL_VERSION" ]; then
    echo "Anemone toolchain is already installed."
    exit 0
fi

"$BUSYBOX" mkdir -p "$ROOT/etc" "$ROOT/root" "$ROOT/usr/bin" "$ROOT/usr/sbin"
"$BUSYBOX" ln -sfn /usr/bin "$ROOT/bin"
"$BUSYBOX" ln -sfn /usr/sbin "$ROOT/sbin"

for applet in sh ls init; do
    echo "Installing $applet..."
    "$BUSYBOX" ln -sf /home/bin/busybox "$ROOT/usr/bin/$applet"
done

"$BUSYBOX" cp -Rf /home/etc/. "$ROOT/etc/"
"$BUSYBOX" cp -Rf /home/root/. "$ROOT/root/"
"$BUSYBOX" rm -f "$ROOT/usr/sbin/init"
"$BUSYBOX" ln -s /usr/bin/init "$ROOT/usr/sbin/init"
"$BUSYBOX" ln -sf /home/sbin/shutdown "$ROOT/usr/bin/poweroff"
echo "$INSTALL_VERSION" > "$INSTALL_MARKER"
"$BUSYBOX" rm -f "$REINSTALL_MARKER"

echo "Anemone toolchain is ready."
