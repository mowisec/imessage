# 02 — Rustpush Measurement Clients

Paper sections: §4.2.4 Capability Harvesting, §4.3.3 Device Targeted Messaging, §4.4 User Handle Leakage from IDS, §5 Device Activity Monitoring, §6 Geolocation Fingerprinting, §7.3 Identity Services Rate Limiting

## Summary

This is our customised iMessage client: a standalone implementation of Apple's iMessage/IDS/APS protocols that lets us query Apple's Identity Service and send messages. Note, requires an account signed-in via OpenBubbles.

[OpenBubbles/rustpush](https://github.com/OpenBubbles/rustpush) is a portable library implementing Apple's iMessage/IDS/APS protocols. We customized its implementation and added stand alone command line clients on top (`imessage-sender/`).

**Tested on Ubuntu 24.04** (see `docker/Dockerfile` / `docker/compose.yaml`). This is the configuration we recommend, use the provided Docker image if possible.
We have faced issues running it on other OS versions or non-`x86_64` architecture.

### Which binary reproduces which result

| Paper result | Binary | Notes |
|---|---|---|
| §4.2.4 / Table 2 — harvested capabilities per device | `imessage-query` | Logs IDS responses to SQLite; convert with `../03-capabilities/scripts/json_to_cap_matrix.py`. |
| §4.3.3 — targeted messaging to one device of a user | `imessage-sender-target`, `imessage-sender --push-token` | Encrypts and delivers to selected push tokens only. |
| §4.4 — cross-matching e-mail and phone handles | `imessage-query` | Identical device tokens/identity keys across handles reveal the same Apple Account. |
| §5.2 — silent pings, delivery receipts, message-type survey | `imessage-send-typing`, `imessage-send-message-types`, `imessage-send-reaction` | `--no-force-delivery-receipt` toggles the `setWantsDeliveryStatus` behaviour of §5.2. |
| §5.2.1 / Figures 6–7 — RTT vs. device state | `imessage-listen` + `imessage-send-typing --repeat --delay` | Pair with the ESP32 state automation in [`../04-esp-automation`](../04-esp-automation). |
| §6 / Tables 3–4 — RTT from distributed vantage points | same as above, run inside the Docker image on EC2 | The `boto3` orchestration across the 34 AWS regions is not part of this artifact. |
| §6.2.2 — resource exhaustion | `imessage-sender --xml --repeat-count --repeat-delay-ms` | Send the empty-`t` / oversized-`x` message of Listing 1 repeatedly. |
| §7.3 — IDS rate limiting | `imessage-query --handles-file --chunk-size` | We recorded only whether limits triggered and stored no account data. |

## Credentials

Rustpush requires an already-registered iMessage identity, provided as a directory (referred to below as `<bluebubbles_dir>`) containing:

- `hw_info.plist` — serialized `HwInfo` (OS config, APS push state, device identity)
- `id.plist` — serialized `Vec<IDSUser>` (the registered IDS identities/keys)

To obtain these files, register a test Apple ID for iMessage using
**[OpenBubbles](https://openbubbles.app/)**, an open-source BlueBubbles-compatible iMessage client.
After registering an account with OpenBubbles, export/copy its generated `hw_info.plist` and `id.plist` into a local directory and pass that directory as `<bluebubbles_dir>`. 

## Building

All binaries require the `macos-validation-data` feature (this pulls in `open-absinthe`, which is a closed-source dependency; the version vendored in this repo (`open-absinthe/`) is a mock placeholder sufficient to compile and run the artifact, see `open-absinthe/README.md`).

```bash
cd rustpush
cargo build --release --features macos-validation-data
```

### Building with Docker (recommended)

```bash
cd docker
docker compose build
```

This builds an `imessage:ubuntu-24.04` image from `docker/Dockerfile` (Ubuntu 24.04 base). Place a built binary (or build inside the container) under `docker/mount/` — this directory is bind-mounted to `/app` in the `imessage-interactive` service defined in `docker/compose.yaml`.

To get a shell in the container with the mount available:

```bash
docker compose run --rm imessage-interactive bash
```

## Executables

All binaries below are located under `imessage-sender/` and require a `<bluebubbles_dir>` as described in [Credentials](#credentials). Most support `--ipc-socket <path>` to forward the request to an already-running `imessage-listen` process (recommended, since only one process can hold the APS connection for a given identity at a time) and `--no-ipc` to instead connect directly.

| Binary | Purpose |
|---|---|
| `imessage-listen` | Long-running listener: opens the APS connection, receives messages, and optionally serves other tools over an IPC socket. |
| `imessage-sender` | Send a plain text (or raw XML) iMessage to a recipient. |
| `imessage-send-typing` | Send/stop a typing indicator to a recipient. |
| `imessage-send-message-types` | Send a fixed suite of message types (used for protocol/feature evaluation). |
| `imessage-send-reaction` | Send a tapback/reaction to a message. |
| `imessage-sender-target` | Send a message to specific target device push tokens. |
| `imessage-query` | Query IDS for the registration status/capabilities of a list of handles, logging results to a SQLite DB. |
| `rustpush-test` | Ad-hoc integration/demo binary exercising most library features (`src/test.rs`). |

### Usage

Start a listener first (keeps the APS connection open):

```bash
./imessage-listen ~/bluebubbles_dir
# or with a custom IPC socket / db / download dir:
./imessage-listen ~/bluebubbles_dir --ipc-socket /tmp/imessage.sock --db ./messages.db --download-dir ./downloads
```

Send a text message:

```bash
./imessage-sender <recipient> "<message>" ~/bluebubbles_dir \
  [--ipc-socket <path>] [--no-ipc] [--push-token <hex>] [--xml <body>] \
  [--repeat-count <n>] [--repeat-delay-ms <ms>]
```

Send/stop a typing indicator:

```bash
./imessage-send-typing ~/bluebubbles_dir <target_handle> \
  [--stop] [--listen] [--ipc-socket <path>] [--no-ipc] [--push-token <hex>] \
  [--repeat <count>] [--delay <ms>] [--no-force-delivery-receipt]
```

Send the evaluation message-type suite:

```bash
./imessage-send-message-types ~/bluebubbles_dir <target_handle> \
  [--stop] [--listen] [--ipc-socket <path>] [--no-ipc] [--push-token <hex>] [--message-index <n>]
```

Send a reaction:

```bash
./imessage-send-reaction ~/bluebubbles_dir <target_handle> "<xml text>" \
  [--amt <number>] [--no-amt] [--read-from-file <path>] [--no-ipc]
```

Query handle registration/capabilities:

```bash
./imessage-query ~/bluebubbles_dir <sqlite_db_path> \
  [--handles-file <path>] [--chunk-size <n>] [--chunk-timeout-seconds <n>] [--random] [handle ...]
```

Run each binary with no arguments (or `--help` where supported) to print its full usage text.

## Repository layout

- `src/` — the `rustpush` library.
- `imessage-sender/` — CLI tools built on the library (see [Executables](#executables)).
- `apple-private-apis/` — vendored GSA authentication / Anisette generation crates.
- `open-absinthe/` — mock placeholder for the closed-source Apple validation-data crate.
- `cloudkit-proto/`, `cloudkit-derive/` — CloudKit protocol support used by `rustpush-test`.
- `certs/` — Apple root certificates required at build/runtime.
- `docker/` — Ubuntu 24.04 build/runtime environment used for artifact evaluation.

