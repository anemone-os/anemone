#!/home/bin/busybox sh

echo Initializing Anemone userspace...
set -eu

BUSYBOX=/home/bin/busybox

"$BUSYBOX" mkdir -p /dev /mnt /proc /root /run /tmp

export PATH=/bin:/sbin:/usr/bin:/usr/sbin
export HOME=/root
export TERM=linux
export LD_LIBRARY_PATH=/lib:/usr/lib

"$BUSYBOX" mount -n -t devfs devfs /dev
# Anemone devfs currently exposes fixed device nodes but rejects mkdir. Restore
# /dev/shm once devfs provides a mountpoint or supports directory creation.
echo "Skipping /dev/shm: devfs does not support directory creation."
"$BUSYBOX" mount -n -t ramfs none /run
"$BUSYBOX" mount -n -t ramfs none /tmp
"$BUSYBOX" mount -n -t proc proc /proc
"$BUSYBOX" chmod 1777 /tmp

echo "Anemone userspace is ready."
"$BUSYBOX" cat /etc/logo.txt
