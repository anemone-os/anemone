#!/busybox sh

# set -x

echo Installing Anemone userspace into rootfs...
# BusyBox 1.33.1 hush rejects `set -u`; restore nounset only after /bin/sh
# switches to ash or the selected hush implementation supports it.
set -e

BUSYBOX=/busybox

"$BUSYBOX" mkdir -p /bin /dev /dev/shm /etc /mnt /proc /root /run /tmp /usr /var
"$BUSYBOX" --install -s /bin

if [ -f /symlinks.sh ]; then
    "$BUSYBOX" sh /symlinks.sh
fi

for link in /sbin /usr/bin /usr/sbin; do
    if [ ! -e "$link" ]; then
        "$BUSYBOX" ln -s /bin "$link"
    fi
done

# Folder-based roots may omit the standard /usr directories. The Alpine image
# already provides them, so these fallbacks are no-ops for the image base.
for name in include lib libexec share; do
    if [ ! -e "/usr/$name" ]; then
        "$BUSYBOX" ln -s "/$name" "/usr/$name"
    fi
done

export PATH=/bin:/sbin:/usr/bin:/usr/sbin
export HOME=/root
export TERM=linux
export LD_LIBRARY_PATH=/lib:/usr/lib

# Anemone currently represents devfs/procfs pseudo mounts with a null source,
# which BusyBox mount cannot express. Keep their mountpoints ready; the kernel
# has already opened the console fds inherited by this init process.
"$BUSYBOX" mount -n -t ramfs none /dev/shm
"$BUSYBOX" mount -n -t ramfs none /run
"$BUSYBOX" mount -n -t ramfs none /tmp
"$BUSYBOX" chmod 1777 /tmp

if [ -f /tests/try_build.sh ]; then
    echo "Running native GCC/Git smoke test from /tests..."
    if ! (cd /tests && "$BUSYBOX" sh ./try_build.sh); then
        echo "Native GCC smoke test failed; continuing to the interactive shell."
    fi
fi

echo "Anemone userspace is ready."
cat /etc/logo.txt
