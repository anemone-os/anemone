FROM ubuntu:24.04 AS build_qemu
ARG QEMU_VERSION=10.0.5
WORKDIR /build
RUN apt update && apt install -y build-essential \
    wget \
    python3 \
    python3-pip \
    ninja-build \
    pkg-config \
    libglib2.0-dev \
    libslirp-dev \
    git \
    flex \
    bison
RUN wget https://download.qemu.org/qemu-${QEMU_VERSION}.tar.xz
RUN tar xvf qemu-${QEMU_VERSION}.tar.xz
WORKDIR /build/build-qemu
RUN ../qemu-${QEMU_VERSION}/configure --target-list=riscv64-softmmu,loongarch64-softmmu --prefix=/opt/qemu
RUN make -j$(nproc)
RUN make install

FROM ubuntu:24.04 AS build_lwext4_toolchains
ARG RISCV64_LWEXT4_TOOLCHAIN_URL=https://gitlab.educg.net/wangmingjian/os-contest-2024-image/-/raw/master/riscv64-linux-musl-cross.tgz
ARG LOONGARCH64_LWEXT4_TOOLCHAIN_URL=https://gitlab.educg.net/wangmingjian/os-contest-2024-image/-/raw/master/loongarch64-linux-musl-cross.tgz
WORKDIR /tmp/toolchains
RUN apt update && apt install -y \
    ca-certificates \
    wget \
    tar
RUN mkdir -p /opt/toolchains && \
    wget -O riscv64-linux-musl-cross.tgz "$RISCV64_LWEXT4_TOOLCHAIN_URL" && \
    tar -xzf riscv64-linux-musl-cross.tgz -C /opt/toolchains && \
    rm riscv64-linux-musl-cross.tgz && \
    wget -O loongarch64-linux-musl-cross.tgz "$LOONGARCH64_LWEXT4_TOOLCHAIN_URL" && \
    tar -xzf loongarch64-linux-musl-cross.tgz -C /opt/toolchains && \
    rm loongarch64-linux-musl-cross.tgz

FROM ubuntu:24.04 AS fin_dev
RUN apt update && apt install -y \
    build-essential \
    python3 \
    python3-pip \
    git \
    openssh-client \
    curl \
    cmake \
    libclang-dev \
    libguestfs-tools \
    linux-image-kvm \
    libglib2.0-0 \
    libslirp0 \
    u-boot-tools \
    sudo
COPY --from=build_lwext4_toolchains /opt/toolchains /opt/toolchains
ENV LWEXT4_TOOLCHAIN_RISCV64=/opt/toolchains/riscv64-linux-musl-cross \
    LWEXT4_TOOLCHAIN_LOONGARCH64=/opt/toolchains/loongarch64-linux-musl-cross 
# Install Rust in a shared location accessible by all users
# RUSTUP_HOME and binaries are shared, but each user gets their own ~/.cargo for registry cache
ENV RUSTUP_HOME=/opt/rust/rustup \
    CARGO_HOME=/opt/rust/cargo \
    PATH="/opt/rust/cargo/bin:${PATH}"
RUN mkdir -p /opt/rust/cargo /opt/rust/rustup && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --no-modify-path && \
    rustup component add llvm-tools-preview && \
    cargo install cargo-fuzz just just-lsp cargo-binutils && \
    chmod -R a+rwX /opt/rust
COPY --from=build_qemu /opt/qemu /opt/qemu
ENV PATH="/opt/qemu/bin:${PATH}"
# Unset CARGO_HOME so users default to ~/.cargo for registry/cache (avoids permission issues)
ENV CARGO_HOME=

ARG USERNAME=user
ARG USER_UID=1000
ARG USER_GID=1000
# Ubuntu development images may already reserve UID/GID 1000, for example for
# an `ubuntu` account. Reuse and rename that account/group when present so the
# dev container has a stable username while still matching the host UID/GID.
RUN set -eux; \
    if getent group "$USERNAME" >/dev/null; then \
    groupmod --gid "$USER_GID" "$USERNAME"; \
    elif getent group "$USER_GID" >/dev/null; then \
    groupmod --new-name "$USERNAME" "$(getent group "$USER_GID" | cut -d: -f1)"; \
    else \
    groupadd --gid "$USER_GID" "$USERNAME"; \
    fi; \
    if id -u "$USERNAME" >/dev/null 2>&1; then \
    usermod --uid "$USER_UID" --gid "$USER_GID" --home "/home/$USERNAME" --move-home "$USERNAME"; \
    elif getent passwd "$USER_UID" >/dev/null; then \
    usermod --login "$USERNAME" --gid "$USER_GID" --home "/home/$USERNAME" --move-home "$(getent passwd "$USER_UID" | cut -d: -f1)"; \
    else \
    useradd --uid "$USER_UID" --gid "$USER_GID" --create-home --shell /bin/bash "$USERNAME"; \
    fi; \
    usermod --shell /bin/bash "$USERNAME"; \
    mkdir -p "/home/$USERNAME"; \
    chown -R "$USER_UID:$USER_GID" "/home/$USERNAME"; \
    echo "$USERNAME ALL=(ALL) NOPASSWD:ALL" > "/etc/sudoers.d/$USERNAME" && \
    chmod 0440 "/etc/sudoers.d/$USERNAME"
USER $USERNAME
WORKDIR /home/$USERNAME
ENTRYPOINT [ "bash" ]

FROM ubuntu:24.04 AS fin_ci
RUN apt update && apt install -y \
    build-essential \
    python3 \
    python3-pip \
    git \
    curl \
    cmake \
    libclang-dev \
    libguestfs-tools \
    linux-image-kvm \
    libglib2.0-0 \
    libslirp0 \
    u-boot-tools \
    sudo

COPY --from=build_lwext4_toolchains /opt/toolchains /opt/toolchains
ENV LWEXT4_TOOLCHAIN_RISCV64=/opt/toolchains/riscv64-linux-musl-cross \
    LWEXT4_TOOLCHAIN_LOONGARCH64=/opt/toolchains/loongarch64-linux-musl-cross 
#    LIBCLANG_PATH=/usr/lib/llvm-18/lib

COPY --from=build_qemu /opt/qemu /opt/qemu
ENV PATH="/opt/qemu/bin:${PATH}"

ENV RUSTUP_HOME=/opt/rust/rustup \
    CARGO_HOME=/opt/rust/cargo \
    PATH="/opt/rust/cargo/bin:${PATH}"
RUN mkdir -p /opt/rust/cargo /opt/rust/rustup && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --no-modify-path && \
    rustup component add llvm-tools-preview && \
    cargo install just cargo-binutils && \
    chmod -R a+rwX /opt/rust
ENV CARGO_HOME=
ENTRYPOINT [ "bash" ]
