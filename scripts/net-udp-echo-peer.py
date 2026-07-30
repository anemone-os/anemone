#!/usr/bin/env python3

import socket
import sys


# These values are the Stage 5 validation wire agreement with udp-test. Keep
# both endpoints in sync; this helper remains the canonical external-path peer.
HOST = "127.0.0.1"
PORT = 49153
REQUEST = b"anemone-udp-stage5-request"
ACK = b"anemone-udp-stage5-ack"
RECEIVE_TIMEOUT_SECONDS = 300.0


def fail(reason: str) -> int:
    print(f"UDPPEER:FAIL:{reason}", flush=True)
    return 1


def main() -> int:
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as peer:
            peer.bind((HOST, PORT))
            peer.settimeout(RECEIVE_TIMEOUT_SECONDS)
            print(f"UDPPEER:READY:{PORT}", flush=True)

            request, source = peer.recvfrom(2048)
            if request != REQUEST:
                return fail("unexpected-token")
            if peer.sendto(ACK, source) != len(ACK):
                return fail("short-send")

            print("UDPPEER:PASS:remote-external-roundtrip", flush=True)
            return 0
    except TimeoutError:
        return fail("timeout")
    except OSError as error:
        return fail(f"socket-errno-{error.errno}")


if __name__ == "__main__":
    sys.exit(main())
