mkdir -vp ${CARGO_HOME:-$HOME/.cargo}

cat << EOF | tee ${CARGO_HOME:-$HOME/.cargo}/config.toml
[source.crates-io]
replace-with = 'mirror'

[source.mirror]
registry = "https://mirrors.aliyun.com/crates.io-index/"
EOF