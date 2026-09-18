# Blue Bubbles, Red Flags: Investigating Privacy Leakage in Apple iMessage

Artifact for the CCS 2026 paper *Blue Bubbles, Red Flags: Investigating Privacy Leakage in Apple iMessage* by Viktor E. Garske, Swantje Lange, Gabriel K. Gegenhuber, David Schmidt, Andreas Noack and Jiska Classen.

The paper presents a systematic analysis of the iMessage protocol implementation with a focus on privacy protections and information leakage. It shows that attackers can fingerprint the device type and OS version of each iMessage client, deliver messages to individual devices of a user, silently infer a client's online status and screen state from delivery receipts, and derive a client's coarse location. This repository contains the tooling behind those four results.

## Directory Structure

```
.
├── 01-frida-scripts/     Dynamic instrumentation of the iMessage subsystem (§3, §4.3.3, §5.1, §7.2)
├── 02-rustpush/          Customised standalone iMessage/IDS client used for all measurements
├── 03-capabilities/      Capability extraction from IPSW firmware + fingerprinting analysis (§4)
└── 04-esp-automation/    ESP32 device-state automation and server-location evaluation (§5, §6)
```

Each folder has its own `README.md` with a description of the experiment, its prerequisites, and step-by-step instructions for reproducing the corresponding results.

## How to Cite

```bibtex
@inproceedings{garske2026bluebubbles,
  author    = {Garske, Viktor E. and Lange, Swantje and Gegenhuber, Gabriel K. and Schmidt, David and Noack, Andreas and Classen, Jiska},
  title     = {Blue Bubbles, Red Flags: Investigating Privacy Leakage in Apple iMessage},
  booktitle = {Proceedings of the ACM SIGSAC Conference on Computer and Communications Security (CCS)},
  year      = {2026},
  address   = {The Hague, Netherlands},
  publisher = {ACM},
  numpages  = {15},
  isbn      = {979-8-4007-2871-6},
  doi       = {10.1145/3830454.3832689}
}
```

## Contact

* Viktor E. Garske — viktor.garske@ismk-stralsund.de (ISMK, University of Applied Sciences Stralsund)
* Swantje Lange — swantje.lange@hpi.de (Hasso Plattner Institute, University of Potsdam)
* Gabriel K. Gegenhuber — gabriel.gegenhuber@it-u.at (Interdisciplinary Transformation University Linz)
* David Schmidt — d.schmidt@univie.ac.at (University of Vienna, CDL AsTra)
* Andreas Noack — andreas.noack@ismk-stralsund.de (ISMK, University of Applied Sciences Stralsund)
* Jiska Classen — jiska.classen@hpi.de (Hasso Plattner Institute, University of Potsdam)

