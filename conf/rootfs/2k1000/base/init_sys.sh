#!/home/bin/busybox sh

echo Initializing Alpine userspace for Anemone...
set -eu

BUSYBOX=/home/bin/busybox

/home/install_sys.sh /

"$BUSYBOX" mkdir -p /dev /mnt /proc /root /run /sys /tmp

export PATH=/bin:/sbin:/usr/bin:/usr/sbin
export HOME=/root
export TERM=linux
export LD_LIBRARY_PATH=/lib:/usr/lib

"$BUSYBOX" mount -n -t devfs devfs /dev
# Anemone devfs publishes /dev/pts and /dev/ptmx; devpts provides the
# per-session slave nodes used by OpenSSH and other terminal applications.
"$BUSYBOX" mount -n -t devpts devpts /dev/pts
"$BUSYBOX" mount -n -t ramfs none /run
"$BUSYBOX" mkdir -p \
    /run/lock \
    /run/nginx \
    /run/openrc \
    /run/user \
    /run/user/0
"$BUSYBOX" chmod 0755 \
    /run \
    /run/lock \
    /run/nginx \
    /run/openrc \
    /run/user
"$BUSYBOX" chmod 0700 /run/user/0
"$BUSYBOX" mount -n -t ramfs none /tmp
"$BUSYBOX" mount -n -t proc proc /proc
"$BUSYBOX" mount -n -t sysfs sysfs /sys
# Anemone devfs provides /dev/shm but rejects mkdir inside /dev.
"$BUSYBOX" mount -n -t tmpfs tmpfs /dev/shm
"$BUSYBOX" chmod 1777 /tmp /dev/shm

echo "Anemone userspace is ready."
if [ -f /etc/logo.txt ]; then
    "$BUSYBOX" cat /etc/logo.txt
fi
