#!/usr/bin/env python3
"""Bounded host peer for the net-tcp Stage 5 remote-external oracle."""

import argparse
import signal
import socket
import struct
import sys
import time


REQUEST = b"ANEMONE_TCP_STAGE5_REQUEST\0"
RESET_REQUEST = b"ANEMONE_TCP_STAGE5_RESET__\0"
REPLY = b"ANEMONE_TCP_STAGE5_REPLY\0"


def receive_exact(connection: socket.socket, length: int) -> bytes:
    received = bytearray()
    while len(received) < length:
        chunk = connection.recv(length - len(received))
        if not chunk:
            raise RuntimeError("peer observed EOF before the complete request")
        received.extend(chunk)
    return bytes(received)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--sessions", type=int, required=True)
    parser.add_argument("--timeout", type=float, default=180.0)
    args = parser.parse_args()
    if not 0 < args.port < 65536 or args.sessions <= 0 or args.timeout <= 0:
        parser.error("port, sessions, and timeout must be positive and bounded")

    stop = False

    def request_stop(_signum: int, _frame: object) -> None:
        nonlocal stop
        stop = True

    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)
    deadline = time.monotonic() + args.timeout

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", args.port))
        listener.listen(args.sessions)
        listener.settimeout(0.25)
        print(
            f"TCPSTAGE5:PEER:READY:port={args.port}:sessions={args.sessions}",
            flush=True,
        )

        completed = 0
        while completed < args.sessions and not stop:
            if time.monotonic() >= deadline:
                raise TimeoutError(
                    f"peer timed out after {completed}/{args.sessions} sessions"
                )
            try:
                connection, peer = listener.accept()
            except socket.timeout:
                continue
            with connection:
                connection.settimeout(min(30.0, args.timeout))
                if peer[0] != "127.0.0.1" or peer[1] == 0:
                    raise RuntimeError(f"unexpected QEMU proxy peer tuple: {peer!r}")
                request = receive_exact(connection, len(REQUEST))
                if request == REQUEST:
                    kind = "stream"
                    connection.sendall(REPLY)
                    connection.shutdown(socket.SHUT_WR)
                    if connection.recv(1) != b"":
                        raise RuntimeError("guest sent bytes after its FIN")
                elif request == RESET_REQUEST:
                    kind = "reset"
                    connection.setsockopt(
                        socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0)
                    )
                else:
                    raise RuntimeError(f"unexpected request: {request!r}")
                completed += 1
                print(
                    f"TCPSTAGE5:PEER:SESSION:PASS:{completed}:kind={kind}:peer={peer[0]}:{peer[1]}",
                    flush=True,
                )

        if stop:
            raise RuntimeError("peer interrupted before all sessions completed")
        print(f"TCPSTAGE5:PEER:SUMMARY:PASS:{completed}", flush=True)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"TCPSTAGE5:PEER:FAIL:{error}", file=sys.stderr, flush=True)
        raise SystemExit(1)
