# 01 — Frida Scripts (iMessage Client Instrumentation)

Paper sections: §3 Threat Model, §4.3.3 Device Targeted Messaging, §5.1 Methodology, §7.2 Effectiveness of BlastDoor

## Summary

These Frida scripts dynamically instrument Apple's iMessage subsystem (`imagent`, `apsd`, `IMTransferAgent`, and the `Messages` app) to reverse-engineer how messages, delivery receipts, and attachments are constructed before they leave the device. They are the tooling behind our manual protocol analysis: they let us observe the `setWantsDeliveryStatus` option, craft *silent* messages that are acknowledged by delivery receipts but not displayed in the UI, and inspect what BlastDoor accepts or rejects.

## Requirements

* **A jailbroken iOS/iPadOS device** (we used a jailbroken iPad running iPadOS 18.6) **or a SIP-disabled macOS machine**.
  * We used an iPhone 8 with the Palera1n jailbreak (https://github.com/palera1n/palera1n) and a SIP-disabled Mac mini as testing devices. For Disabling SIP: boot into recovery, `csrutil disable`, reboot.
* Install **Frida** on the host: `pip install frida-tools`. We used verison 17.5.2.
* `frida-server` needs to run on the jailbroken device (iOS), reachable via USB (`frida-ls-devices` should list it).
* We strictly recommend to only use dedicated testing Apple IDs and devices.

## Running the scripts

Each script carries a comment on its first line naming the process it must be attached to. Attach with:

```bash
# on a jailbroken iOS device connected over USB (-U)
frida -U -n imagent -l intercept_imessage.js

# on a SIP-disabled Mac (local device)
frida -n imagent -l intercept_imessage.js
```

Replace `-n imagent` with the target process from the table below. 

| Script | Attach to | What it does |
|---|---|---|
| `intercept_imessage.js` | `imagent` | Dumps incoming and outgoing top-level iMessage payloads (the decrypted message dictionary, incl. the `t` and `x` fields). Main observation script for §5.2. |
| `intercept_messages_apsd.js` | `apsd` | Dumps `APSMessage` objects at the still-encrypted push layer, showing what arrives from APNs before iMessage decoding. |
| `modify_messages.js` | `imagent` | Rewrites the `t` (plain text) and `x` (styled XML) fields of an outgoing message. Used to build the empty/styled-text messages of Listing 1 and the oversized messages of the resource-exhaustion experiment (§6.2.2). Edit `newT` / `newX` at the top of the file before loading. |
| `social_probe_lockdown_mode.js` | `imagent` | Variant of `modify_messages.js` that sends a `texteffect`-only message with an empty `t` field, i.e., the *silent ping* primitive used against a device in Lockdown Mode. |
| `enforce_delivery_receipt.js` | `imagent` | Forces `-[IDSSendParameters setWantsDeliveryStatus:]` to `true` for every message, so that ephemeral message types (typing indicators, receipts) also produce a delivery receipt. This is the hook that enables silent pinging (§5.2). |
| `overwrite_has_recently_messaged.js` | `imagent` | Forces `_hasRecentlyMessaged:` / `hasRecentlyMessaged:` to return true, bypassing the client-side check that suppresses sending to handles with no prior contact. |
| `probing_helper.js` | `imagent` | Timestamps outgoing `IDSSendParameters` and the matching delivery receipts, yielding the RTT samples analysed in §5.2.1 and §6. |
| `inject_messages.js` | `Messages` | Constructs and sends `IMMessage` objects programmatically, incl. creating the chat if it does not exist. Combine with `modify_messages.js` to alter the payload before it is encrypted. |
| `init_chat.js` | `Messages` | Minimal example of creating an `IMChat` for a handle via `IMChatRegistry`. Set the handle string (line 11) before running. |
| `query_reachability.js` | `Messages` | Queries `IMServiceReachabilityController` for whether a handle is reachable over iMessage; used while exploring IDS lookups (§4.4). Call `queryReachability("mailto:…")` or `queryReachability("tel:+…")` from the REPL. |
| `modify_attachments.js` | `IMTransferAgent` | Intercepts `-[IMTransferAgentController sendFilePath:…]` to swap the file being uploaded and to toggle iCloud encryption. Supports the attachment-based location leak (§6.2.1). Set `newPath` (line 21) to an existing file. |
| `sniff_https.js` | `IMTransferAgent` | Logs `NSURLSession` requests, headers and response bodies during attachment up-/download, exposing the `x-apple-edge-info` and `X-Amz-Credential` metadata discussed in §6.2.1. |
| `sandbox_checks.js` | any process (e.g. `launchd`) | Traces `sandbox_check` / `sandbox_check_by_audit_token` to follow XPC connection establishment between the iMessage daemons (§2, Figure 2). |

## Notes

* Hooked selectors are private API and change across OS releases. The scripts were last exercised against iOS/iPadOS 18.6 and macOS 15.6; on other versions a selector may need to be re-resolved.
* Several scripts contain hard-coded placeholders (handles, file paths, `mmcs-url` values) that were redacted or are specific to our test accounts. Search for empty string literals and adjust them to your own test setup before running.
