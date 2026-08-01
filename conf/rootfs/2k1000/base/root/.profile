# /etc/profile owns the login-shell prompt and account environment.
if [ -f "$HOME/.cargo/env" ]; then
    . "$HOME/.cargo/env"
fi
