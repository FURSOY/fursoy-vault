import argparse
import json
import os
import struct
import subprocess
import sys
import tempfile

EXTENSION_ID = "ibjddphkjppgkdbegjibddbjkagdlaea"
ORIGIN = f"chrome-extension://{EXTENSION_ID}/"
CAPABILITIES = ["chunked_cookies", "request_correlation", "config_v3", "audit_recovery", "profile_namespace"]
V7_CAPABILITIES = CAPABILITIES + ["durable_operations_v7", "guarded_cookie_removal", "semantic_operation_status", "profile_recovery_v1"]
PROFILE_A = "11111111-1111-4111-8111-111111111111"
PROFILE_B = "22222222-2222-4222-8222-222222222222"


def write_message(stream, message):
    body = json.dumps(message, separators=(",", ":")).encode()
    stream.write(struct.pack("<I", len(body)) + body)
    stream.flush()


def read_message(stream):
    prefix = stream.read(4)
    if len(prefix) != 4:
        return None
    size = struct.unpack("<I", prefix)[0]
    return json.loads(stream.read(size))


def exchange(executable: str, origin: str, protocol: int, data_dir: str, profile_id: str = PROFILE_A, capabilities=None):
    version = "0.5.0" if protocol == 7 else "0.4.1"
    message = {
        "v": protocol,
        "conn_nonce": "42" * 32,
        "seq": 1,
        "id": "42424242-4242-4242-8242-424242424242",
        "type": "handshake",
        "payload": {
            "protocol_version": protocol,
            "extension_id": EXTENSION_ID,
            "profile_id": profile_id,
            "extension_version": version,
            "min_host_version": version,
            "capabilities": capabilities if capabilities is not None else (V7_CAPABILITIES if protocol == 7 else CAPABILITIES),
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


def attempt_cross_profile_claim(executable: str, data_dir: str, source_id: str, current_id: str):
    environment = {**os.environ, "FCP_DATA_DIR": data_dir}
    process = subprocess.Popen(
        [executable, ORIGIN, "--parent-window=0"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
    )
    nonce = "43" * 32
    handshake = {
        "v": 6, "conn_nonce": nonce, "seq": 1,
        "id": "51515151-5151-4151-8151-515151515151", "type": "handshake",
        "payload": {
            "protocol_version": 6, "extension_id": EXTENSION_ID, "profile_id": current_id,
            "extension_version": "0.4.1", "min_host_version": "0.4.1",
            "capabilities": CAPABILITIES, "cached_config_digest": None,
        },
    }
    write_message(process.stdin, handshake)
    ack = read_message(process.stdout)
    assert ack and ack["type"] == "handshake.ack"
    claim = {
        "v": 6, "conn_nonce": nonce, "seq": 2,
        "id": "52525252-5252-4252-8252-525252525252", "type": "profile.recovery.claim",
        "payload": {"source_profile_id": source_id, "target_profile_id": "44444444-4444-4444-8444-444444444444"},
    }
    write_message(process.stdin, claim)
    result = read_message(process.stdout)
    process.stdin.close()
    code = process.wait(timeout=15)
    error = process.stderr.read().decode(errors="replace")
    return code, result, error


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("executable")
    arguments = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="fursoy-acceptance-") as data_dir:
        code, response, error = exchange(arguments.executable, ORIGIN, 6, data_dir)
        assert code == 0, error
        assert response and response["type"] == "handshake.ack"
        assert response["v"] == 6 and response["id"] == "42424242-4242-4242-8242-424242424242"
        assert response["payload"]["protocol_version"] == 6
        assert set(CAPABILITIES).issubset(response["payload"]["capabilities"])

    # The same native host/data root must expose different authoritative configs to different
    # browser profiles. Chrome itself isolates cookie stores; this verifies our disk vault follows
    # the same boundary instead of reusing one profile's encrypted session in another profile.
    with tempfile.TemporaryDirectory(prefix="fursoy-acceptance-profiles-") as data_dir:
        profile_config = os.path.join(data_dir, "profiles", PROFILE_A, "config")
        os.makedirs(profile_config)
        with open(os.path.join(profile_config, "account-groups.json"), "w", encoding="utf-8") as file:
            json.dump({
                "version": 3,
                "compatibility_version": 3,
                "groups": [
                    {
                        "id": "33333333-3333-4333-8333-333333333333",
                        "display_name": "Profile A One",
                        "scope": "example.com",
                        "policy_level": "balanced",
                        "store_policy": "normal_profile",
                    },
                    {
                        "id": "55555555-5555-4555-8555-555555555555",
                        "display_name": "Profile A Two",
                        "scope": "example.org",
                        "policy_level": "critical",
                        "store_policy": "normal_profile",
                    },
                ],
            }, file)
        code_a, response_a, error_a = exchange(arguments.executable, ORIGIN, 6, data_dir, PROFILE_A)
        code_b, response_b, error_b = exchange(arguments.executable, ORIGIN, 6, data_dir, PROFILE_B)
        assert code_a == 0, error_a
        assert code_b == 0, error_b
        assert len(response_a["payload"]["config"]["groups"]) == 2
        assert response_b["payload"]["config"]["groups"] == []
        assert "recovery_profiles" not in response_b["payload"]

        claim_code, claim_result, _ = attempt_cross_profile_claim(
            arguments.executable, data_dir, PROFILE_A, PROFILE_B
        )
        assert claim_code != 0 and claim_result is None
        assert os.path.exists(os.path.join(data_dir, "profiles", PROFILE_A, "config", "account-groups.json"))
        assert not os.path.exists(os.path.join(data_dir, "profiles", "44444444-4444-4444-8444-444444444444"))

    with tempfile.TemporaryDirectory(prefix="fursoy-acceptance-origin-") as data_dir:
        code, response, _ = exchange(arguments.executable, "chrome-extension://unauthorized/", 6, data_dir)
        assert code != 0 and response is None

    # Rollout is host-first safe: a v7 extension built immediately before profile recovery did
    # not offer the new optional capability. The new host still accepts it while advertising the
    # feature to updated extensions.
    with tempfile.TemporaryDirectory(prefix="fursoy-acceptance-recovery-rollout-") as data_dir:
        old_v7 = CAPABILITIES + ["durable_operations_v7", "guarded_cookie_removal", "semantic_operation_status"]
        code, response, error = exchange(
            arguments.executable, ORIGIN, 7, data_dir, PROFILE_A, old_v7
        )
        assert code == 0, error
        assert response and response["type"] == "handshake.ack"
        assert "profile_recovery_v1" in response["payload"]["capabilities"]

    with tempfile.TemporaryDirectory(prefix="fursoy-acceptance-version-") as data_dir:
        code, response, _ = exchange(arguments.executable, ORIGIN, 5, data_dir)
        assert code != 0 and response is None

    with tempfile.TemporaryDirectory(prefix="fursoy-acceptance-v7-floor-") as data_dir:
        profile_config = os.path.join(data_dir, "profiles", PROFILE_A, "config")
        os.makedirs(profile_config)
        with open(os.path.join(profile_config, "account-groups.json"), "w", encoding="utf-8") as file:
            json.dump({"version": 3, "compatibility_version": 3, "groups": [{
                "id": "33333333-3333-4333-8333-333333333333", "display_name": "Floor",
                "scope": "example.com", "policy_level": "balanced", "store_policy": "normal_profile"
            }]}, file)
        code, response, error = exchange(arguments.executable, ORIGIN, 7, data_dir)
        assert code == 0, error
        assert response and response["type"] == "handshake.ack" and response["v"] == 7
        lease_path = os.path.join(data_dir, "profiles", PROFILE_A, "leases", "groups", "33333333-3333-4333-8333-333333333333.json")
        with open(lease_path, encoding="utf-8") as file:
            assert json.load(file)["protocol_floor"] == 7
        _, downgrade, _ = exchange(arguments.executable, ORIGIN, 6, data_dir)
        assert downgrade is None or downgrade["type"] != "handshake.ack"

    print("native handshake acceptance: PASS")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"native handshake acceptance: FAIL: {error}", file=sys.stderr)
        raise
