# TODO
# - LoongArch support
# - Add fin_prod

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
    git \
    flex \
    bison
RUN wget https://download.qemu.org/qemu-${QEMU_VERSION}.tar.xz
RUN tar xvf qemu-${QEMU_VERSION}.tar.xz
WORKDIR /build/build-qemu
RUN ../qemu-${QEMU_VERSION}/configure --target-list=riscv64-softmmu --prefix=/opt/qemu
RUN make -j$(nproc)
RUN make install

FROM ubuntu:24.04 AS build_gnu_riscv
ARG GNU_VERSION=2026.01.09
WORKDIR /build
RUN apt update && apt install -y wget xz-utils
RUN wget https://github.com/riscv-collab/riscv-gnu-toolchain/releases/download/${GNU_VERSION}/riscv64-elf-ubuntu-24.04-gcc.tar.xz
RUN tar xvf riscv64-elf-ubuntu-24.04-gcc.tar.xz

FROM ubuntu:24.04 AS fin_dev
RUN apt update && apt install -y \
    build-essential \
    python3 \
    python3-pip \
    git \
    openssh-client \
    curl \
    libglib2.0-0
# Install Rust in a shared location accessible by all users
# RUSTUP_HOME and binaries are shared, but each user gets their own ~/.cargo for registry cache
ENV RUSTUP_HOME=/opt/rust/rustup \
    CARGO_HOME=/opt/rust/cargo \
    PATH="/opt/rust/cargo/bin:${PATH}"
RUN mkdir -p /opt/rust/cargo /opt/rust/rustup && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --no-modify-path && \
    cargo install cargo-fuzz just just-lsp && \
    chmod -R a+rwX /opt/rust
COPY --from=build_qemu /opt/qemu /opt/qemu
COPY --from=build_gnu_riscv /build/riscv /opt/gnu-riscv
ENV PATH="/opt/gnu-riscv/bin:/opt/qemu/bin:${PATH}"
# Unset CARGO_HOME so users default to ~/.cargo for registry/cache (avoids permission issues)
ENV CARGO_HOME=
ENTRYPOINT [ "bash" ]

FROM ubuntu:24.04 AS fin_ci
RUN apt update && apt install -y \
    build-essential \
    python3 \
    python3-pip \
    curl
ENV RUSTUP_HOME=/opt/rust/rustup \
    CARGO_HOME=/opt/rust/cargo \
    PATH="/opt/rust/cargo/bin:${PATH}"
RUN mkdir -p /opt/rust/cargo /opt/rust/rustup && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --no-modify-path && \
    cargo install just && \
    chmod -R a+rwX /opt/rust
COPY --from=build_gnu_riscv /build/riscv /opt/gnu-riscv
ENV PATH="/opt/gnu-riscv/bin:${PATH}"
ENV CARGO_HOME=
ENTRYPOINT [ "bash" ]
