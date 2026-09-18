# 03 — Capability Extraction and Analysis (Device and OS Fingerprinting)

Paper sections: §4 Device and OS Fingerprinting — Figures 3, 4, 5 and Tables 1, 2

## Summary

This experiment implements the two-stage fingerprinting pipeline of Figure 3: it *extracts* the iMessage capabilities that a firmware image symbolically supports by statically analysing the `IDSFoundation` private framework inside an IPSW's DYLD shared cache, and it *analyses* the resulting dataset together with capabilities *harvested* from Apple's IDS server. 

## Contents

```
03-capabilities/
├── capabilities_dyld.py          # static extraction from one IPSW (Binary Ninja headless)
├── capabilities.sh               # standalone DeviceTree check for supports-uwb
├── scripts/
│   └── json_to_cap_matrix.py     # IDS harvesting JSON -> capability x device CSV matrix
├── capabilities-ipsw/
│   └── capabilities-results/     # our pre-computed results: 116 <IPSW name>/capabilities.json
└── capability-analysis/
    ├── 1_extract_caps.ipynb      # Figures 4 and 5, per-version capability statistics
    ├── 2_fingerprint_devs.ipynb  # Table 2: minimal capability set separating device types
    ├── harvested_devices.csv     # harvested IDS capabilities, our 8 devices (handles redacted)
    └── harvested_visionpro.csv   # harvested IDS capabilities, Vision Pro (consented participant)
```

## Re-running the static extraction from an IPSW

This reproduces the *Capability Extraction* stage (§4.2.3) and is only needed if you want to verify the extraction itself or add new firmware images.

**Requirements**

* **macOS** (the extraction mounts Apple filesystems and `ipsw` relies on it for some image types).
* **Binary Ninja** with a commercial or headless-capable licence and the Python API — the script uses the shared-cache API, which is not available in the free edition. We used version 5.3.8695.
* [`blacktop/ipsw`](https://github.com/blacktop/ipsw): `brew install blacktop/tap/ipsw`
* Roughly 30 GB of free disk space per concurrently processed image (extraction writes to `/tmp/extracted`).

**Download firmware images**

```bash
ipsw download appledb --os iOS    --latest
ipsw download appledb --os iPadOS --latest
ipsw download appledb --os macOS  --latest
```

**Run the extraction**

```bash
export BN_DIR="/Applications/Binary Ninja.app/Contents/Resources"
export PYTHONPATH="$BN_DIR/python:$PYTHONPATH"
export DYLD_FALLBACK_LIBRARY_PATH="$BN_DIR:$DYLD_FALLBACK_LIBRARY_PATH"

python3 capabilities_dyld.py ~/Downloads/iPhone17,1_26.3_23D5089e_Restore.ipsw
```

The script extracts the DYLD shared cache, resolves the `_IDSRegistrationProperty`-prefixed symbols in `IDSFoundation` to obtain the symbolic support $S_{i,c}$, evaluates the statically decidable support functions (`-[FTDeviceSupport supportsUWB]`, `supportsAnimojiV2`, Zelkova, …) to obtain the effective value $X_{d,c}$, and finally consults the DeviceTree for hardware-dependent capabilities. 

Copy that directory into `capabilities-ipsw/capabilities-results/` to feed it into the notebooks. Note that the analysis parses device model and OS version **out of the IPSW filename**, so keep Apple's original `<device>_<version>_<build>_Restore.ipsw` naming.

Analysis of a single image takes on the order of 10–30 minutes, dominated by Binary Ninja loading the shared cache. `arm64_32` caches (older Apple Watch models) are not supported by Binary Ninja; those gaps are the ones we filled through harvesting (§4.2.4).

**`capabilities.sh`** is a self-contained cross-check for one hardware capability: it extracts only the DeviceTree and reports `supports-uwb: 1/0` depending on whether a `rose` device is present. Set the `IPSW` variable at the top of the file, then `bash capabilities.sh`.

## Capability harvesting from IDS

Harvesting queries Apple's IDS server for the capabilities a *live* device advertises. The querying client is `imessage-query` in [`../02-rustpush`](../02-rustpush) — see that README for credentials and build instructions:

```bash
./imessage-query <bluebubbles_dir> harvest.db --handles-file handles.txt
```

Export the per-handle results to JSON and convert them into the capability × device matrix the notebooks expect:

```bash
python3 scripts/json_to_cap_matrix.py harvest.json capability-analysis/mine.csv
```

The converter normalises `true`/`false` to `T`/`F`, renders omitted capabilities as `n/a` (the MNAR signal modelled in §4.2.2), and flags disagreeing observations as `CONFLICT(a|b)`. Point the `dataset_our` variable in `2_fingerprint_devs.ipynb` at your CSV to analyse it.

