#!/busybox sh

set -e
# set -x

BUSYBOX=${BUSYBOX:-/busybox}

ln_sf() {
    target=$1
    link=$2
    dir=${link%/*}
    echo "Creating symlink $link -> $target"
    if [ "$dir" != "$link" ]; then
        "$BUSYBOX" mkdir -p "$dir"
    fi

    if [ ! -e "$link" ] || [ -L "$link" ]; then
        "$BUSYBOX" ln -sf "$target" "$link"
    fi
}

echo "Creating symlinks for Anemone userspace..."

# ln_sf ld-musl-loongarch64.so.1 /lib/libc.musl-loongarch64.so.1
# 
# ln_sf gcc /usr/bin/cc
# ln_sf loongarch64-alpine-linux-musl-gcc /usr/bin/loongarch64-alpine-linux-musl-cc
# ln_sf make /usr/bin/gmake
# 
# ln_sf /usr/libexec/gcc/loongarch64-alpine-linux-musl/14.2.0/liblto_plugin.so /usr/lib/bfd-plugins/liblto_plugin.so
# ln_sf libatomic.so.1.2.0 /usr/lib/libatomic.so
# ln_sf libatomic.so.1.2.0 /usr/lib/libatomic.so.1
# ln_sf libbrotlicommon.so.1.1.0 /usr/lib/libbrotlicommon.so.1
# ln_sf libbrotlidec.so.1.1.0 /usr/lib/libbrotlidec.so.1
# ln_sf libbrotlienc.so.1.1.0 /usr/lib/libbrotlienc.so.1
# ln_sf ../../lib/ld-musl-loongarch64.so.1 /usr/lib/libc.so
# ln_sf libcares.so.2.19.4 /usr/lib/libcares.so.2
# ln_sf libctf-nobfd.so.0.0.0 /usr/lib/libctf-nobfd.so.0
# ln_sf libctf.so.0.0.0 /usr/lib/libctf.so.0
# ln_sf libcurl.so.4.8.0 /usr/lib/libcurl.so.4
# ln_sf libexpat.so.1.10.1 /usr/lib/libexpat.so.1
# ln_sf libgmp.so.10.5.0 /usr/lib/libgmp.so.10
# ln_sf libgomp.so.1.0.0 /usr/lib/libgomp.so
# ln_sf libgomp.so.1.0.0 /usr/lib/libgomp.so.1
# ln_sf libidn2.so.0.4.0 /usr/lib/libidn2.so.0
# ln_sf libisl.so.23.3.0 /usr/lib/libisl.so.23
# ln_sf libjansson.so.4.14.0 /usr/lib/libjansson.so.4
# ln_sf libmpc.so.3.3.1 /usr/lib/libmpc.so.3
# ln_sf libmpfr.so.6.2.1 /usr/lib/libmpfr.so.6
# ln_sf libnghttp2.so.14.28.3 /usr/lib/libnghttp2.so.14
# ln_sf libpcre2-8.so.0.12.0 /usr/lib/libpcre2-8.so.0
# ln_sf libpcre2-posix.so.3.0.5 /usr/lib/libpcre2-posix.so.3
# ln_sf libpsl.so.5.3.5 /usr/lib/libpsl.so.5
# ln_sf libsframe.so.1.0.0 /usr/lib/libsframe.so.1
# ln_sf libstdc++.so.6.0.33 /usr/lib/libstdc++.so
# ln_sf libstdc++.so.6.0.33 /usr/lib/libstdc++.so.6
# ln_sf libunistring.so.5.1.0 /usr/lib/libunistring.so.5
# ln_sf libz.so.1.3.1 /usr/lib/libz.so.1
# ln_sf libzstd.so.1.5.6 /usr/lib/libzstd.so.1
# 
# ln_sf git /usr/bin/git-receive-pack
# ln_sf git /usr/bin/git-upload-archive
# ln_sf git /usr/bin/git-upload-pack
# 
# ln_sf ../../bin/git /usr/libexec/git-core/git
# # for name in \
# #     blame add am annotate apply archive bisect branch bugreport bundle cat-file \
# #     check-attr check-ignore check-mailmap check-ref-format checkout checkout--worker \
# #     checkout-index cherry cherry-pick clean clone column commit commit-graph commit-tree \
# #     config count-objects credential credential-cache credential-cache--daemon \
# #     credential-store describe diagnose diff diff-files diff-index diff-tree difftool \
# #     fast-export fetch fetch-pack fmt-merge-msg for-each-ref for-each-repo format-patch \
# #     fsck fsck-objects fsmonitor--daemon gc get-tar-commit-id grep hash-object help \
# #     hook index-pack init init-db interpret-trailers log ls-files ls-remote ls-tree \
# #     mailinfo mailsplit maintenance merge merge-base merge-file merge-index merge-ours \
# #     merge-recursive merge-subtree merge-tree mktag mktree multi-pack-index mv name-rev \
# #     notes pack-objects pack-redundant pack-refs patch-id prune prune-packed pull push \
# #     range-diff read-tree rebase receive-pack reflog refs remote remote-ext remote-fd \
# #     repack replace replay rerere reset restore rev-list rev-parse revert rm send-pack \
# #     shortlog show show-branch show-index show-ref sparse-checkout stage stash status \
# #     stripspace submodule--helper switch symbolic-ref tag unpack-file unpack-objects \
# #     update-index update-ref update-server-info upload-archive upload-pack var \
# #     verify-commit verify-pack verify-tag version whatchanged worktree write-tree
# # do
# #     ln_sf ../../bin/git "/usr/libexec/git-core/git-$name"
# # done
# 
# ln_sf git-remote-http /usr/libexec/git-core/git-remote-ftp
# ln_sf git-remote-http /usr/libexec/git-core/git-remote-ftps
# ln_sf git-remote-http /usr/libexec/git-core/git-remote-https
