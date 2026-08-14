#!/usr/bin/env python3
"""Hostfwd peer for the TCP listener-ingress runtime oracle."""

import argparse
import socket
import sys
import time


REQUEST = b"ANEMONE_LISTENER_EXTERNAL\0"
REPLY = b"ANEMONE_LISTENER_REPLY\0"
PHASE_PORTS = {"WILDCARD": 26010, "EXTERNAL": 26011}


def receive_exact(connection: socket.socket, length: int) -> bytes:
    received = bytearray()
    while len(received) < length:
        chunk = connection.recv(length - len(received))
        if not chunk:
            raise RuntimeError("guest closed before the complete listener reply")
        received.extend(chunk)
    return bytes(received)


def connect_forwarded(port: int, deadline: float) -> socket.socket:
    while True:
        try:
            connection = socket.create_connection(("127.0.0.1", port), timeout=1.0)
            connection.settimeout(5.0)
            return connection
        except OSError:
            if time.monotonic() >= deadline:
                raise TimeoutError(f"hostfwd tcp::{port}-:{port} did not become reachable")
            time.sleep(0.05)


def positive_ingress(phase: str, deadline: float) -> None:
    port = PHASE_PORTS[phase]
    with connect_forwarded(port, deadline) as connection:
        connection.sendall(REQUEST)
        if receive_exact(connection, len(REPLY)) != REPLY:
            raise RuntimeError(f"{phase} guest returned the wrong reply")
        if connection.recv(1) != b"":
            raise RuntimeError(f"{phase} guest sent bytes after shutdown")


def rejected_loopback_ingress(_deadline: float) -> None:
    port = 26012
    try:
        connection = socket.create_connection(("127.0.0.1", port), timeout=2.0)
    except OSError:
        return
    with connection:
        connection.settimeout(5.0)
        try:
            connection.sendall(REQUEST)
            outcome = connection.recv(1)
        except (ConnectionResetError, BrokenPipeError, OSError):
            return
        if outcome == b"":
            return
        raise RuntimeError("loopback-specific listener accepted external hostfwd ingress")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--control-port", type=int, required=True)
    parser.add_argument("--sessions", type=int, required=True)
    parser.add_argument("--timeout", type=float, default=180.0)
    args = parser.parse_args()
    if not 0 < args.control_port < 65536 or args.sessions <= 0 or args.timeout <= 0:
        parser.error("control port, sessions, and timeout must be positive and bounded")

    deadline = time.monotonic() + args.timeout
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", args.control_port))
        listener.listen(args.sessions)
        listener.settimeout(args.timeout)
        print(
            f"TCPINGRESS:PEER:READY:port={args.control_port}:sessions={args.sessions}",
            flush=True,
        )

        for session in range(1, args.sessions + 1):
            control, peer = listener.accept()
            with control:
                control.settimeout(args.timeout)
                reader = control.makefile("rb", buffering=0)
                for expected in ("WILDCARD", "EXTERNAL", "LOOPBACK"):
                    phase = reader.readline().decode("ascii").rstrip("\n")
                    if phase != expected:
                        raise RuntimeError(f"expected {expected}, received {phase!r}")
                    if phase == "LOOPBACK":
                        rejected_loopback_ingress(deadline)
                    else:
                        positive_ingress(phase, deadline)
                    control.sendall(b"PASS\n")
                    print(
                        f"TCPINGRESS:PEER:PHASE:PASS:session={session}:phase={phase}",
                        flush=True,
                    )
            print(
                f"TCPINGRESS:PEER:SESSION:PASS:{session}:peer={peer[0]}:{peer[1]}",
                flush=True,
            )

    print(f"TCPINGRESS:PEER:SUMMARY:PASS:{args.sessions}", flush=True)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"TCPINGRESS:PEER:FAIL:{error}", file=sys.stderr, flush=True)
        raise SystemExit(1)
