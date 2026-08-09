import argparse
import json
import os
import struct
import subprocess
import sys
import tempfile

EXTENSION_ID = "ikodegbaomnahbjiokfogpedaoifhbde"
ORIGIN = f"chrome-extension://{EXTENSION_ID}/"
CAPABILITIES = ["chunked_cookies", "request_correlation", "config_v3", "audit_recovery"]


def exchange(executable: str, origin: str, protocol: int, data_dir: str):
    message = {
        "v": protocol,
        "conn_nonce": "42" * 32,
        "seq": 1,
        "id": "42424242-4242-4242-8242-424242424242",
        "type": "handshake",
        "payload": {
            "protocol_version": protocol,
            "extension_id": EXTENSION_ID,
            "extension_version": "0.3.1",
            "min_host_version": "0.3.1",
            "capabilities": CAPABILITIES,
            "cached_config_digest": None,
        },
    }
    body = json.dumps(message, separators=(",", ":")).encode()
    environment = {**os.environ, "FCP_DATA_DIR": data_dir}
    process = subprocess.Popen(
        [executable, origin, "--parent-window=0"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
    )
    output, error = process.communicate(struct.pack("<I", len(body)) + body, timeout=15)
    if len(output) < 4:
        return process.returncode, None, error.decode(errors="replace")
    size = struct.unpack("<I", output[:4])[0]
    return process.returncode, json.loads(output[4 : 4 + size]), error.decode(errors="replace")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("executable")
    arguments = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="fursoy-acceptance-") as data_dir:
        code, response, error = exchange(arguments.executable, ORIGIN, 5, data_dir)
        assert code == 0, error
        assert response and response["type"] == "handshake.ack"
        assert response["v"] == 5 and response["id"] == "42424242-4242-4242-8242-424242424242"
        assert response["payload"]["protocol_version"] == 5
        assert set(CAPABILITIES).issubset(response["payload"]["capabilities"])

    with tempfile.TemporaryDirectory(prefix="fursoy-acceptance-origin-") as data_dir:
        code, response, _ = exchange(arguments.executable, "chrome-extension://unauthorized/", 5, data_dir)
        assert code != 0 and response is None

    with tempfile.TemporaryDirectory(prefix="fursoy-acceptance-version-") as data_dir:
        code, response, _ = exchange(arguments.executable, ORIGIN, 4, data_dir)
        assert code != 0 and response is None

    print("native handshake acceptance: PASS")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"native handshake acceptance: FAIL: {error}", file=sys.stderr)
        raise
