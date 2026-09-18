#!/usr/bin/env python3

"""
Collect iMessage capabilities from DYLD shared cache and further IPSW properties.

Runs in Binary Ninja headless mode. To ensure the script runs, export the following paths:

    export BN_DIR="/Applications/Binary Ninja.app/Contents/Resources"
    export PYTHONPATH="$BN_DIR/python:$PYTHONPATH"
    export DYLD_FALLBACK_LIBRARY_PATH="$BN_DIR:$DYLD_FALLBACK_LIBRARY_PATH"

    python3 capabilities_dyld.py [iPhone_xxx.ipsw]

To download a batch of IPSWs, here are some options:

`ipsw download appledb --os iOS --latest` - the latest iOS release for all available iOS devices
`ipsw download appledb --os iPadOS --latest`
`ipsw download appledb --os macOS --latest`

"""

import binaryninja as bn
from binaryninja import sharedcache
from binaryninja.enums import MediumLevelILOperation as M
from pathlib import Path
import sys
import json
import subprocess
import shutil


IPSW = Path(sys.argv[1]).expanduser()
OUT = Path("/tmp/extracted") / IPSW.name
MOUNT = Path("/tmp/mount") / IPSW.name
RESULTS = Path("~/Documents/imessage/capabilities-results").expanduser() / IPSW.name

OUT.mkdir(parents=True, exist_ok=True)
MOUNT.mkdir(parents=True, exist_ok=True)
RESULTS.mkdir(parents=True, exist_ok=True)

print("Analyzing IPSW...")


IS_MACOS = False
if "Mac" in IPSW.name:
    IS_MACOS = True


capabilities = {}

### Start with DYLD Shared Cache Analysis
def extract_dyld_shared_cache_from_ipsw(ipsw: Path) -> str:

    if IS_MACOS:
        subprocess.run(
            ["ipsw", "extract", "--dyld", "--dyld-arch", "arm64e", "-o", str(OUT), ipsw],
            check=True,
        )
    else:
        subprocess.run(
            ["ipsw", "extract", "--dyld", "--dyld-arch", "arm64", "-o", str(OUT), ipsw],
            check=True,
        )

    dyld_files = list(OUT.glob("*/dyld_shared_cache_arm64e"))
    if not dyld_files:
        dyld_files = list(OUT.glob("*/dyld_shared_cache_arm64"))
        if not dyld_files:
            dyld_files = list(OUT.glob("*/dyld_shared_cache_arm64_32"))
            if (dyld_files):
                print("arm64_32 dyld shared cache not supported by binja!")
                return None

    # Pick the first result (there should only be one)
    return str(dyld_files[0])

def load_framework(path: str):
    if IS_MACOS:
        path = path.replace(
            ".framework/",
            ".framework/Versions/A/"
        )
    
    img = dsc.get_image_with_name(path)
    if img is None:
        return False

    dsc.apply_image(bv, img)
    bv.update_analysis_and_wait();

def read_ptr(addr: int) -> int:
    ptr_size = bv.address_size
    return int.from_bytes(bv.read(addr, ptr_size), 'little')

def read_nsint(addr: int) -> int:
    return int.from_bytes(bv.read(addr+0x10, 8), 'little')

def read_nsstring(addr: int) -> int:
        nsstring_len = int.from_bytes(bv.read(addr + 0x18, 8), 'little')
        return bv.read(read_ptr(addr + 0x10), nsstring_len).decode('utf-8')

def read_nsint_return(funname: str) -> int:
    # resolve name -> address
    syms = bv.get_symbols_by_name(funname)
    if syms:
        addr = syms[0].address
        f = bv.get_function_at(addr)
        for insn in f.mlil.instructions:
            if insn.operation == M.MLIL_RET:
                for expr in insn.src:
                    return read_nsint(expr.value.value)
    else:
        return None

def check_return_bool(name: str) -> bool:
    syms = bv.get_symbols_by_name(name)
    if not syms:
        return None
    
    f = bv.get_function_at(syms[0].address)
    block = next(iter(f.basic_blocks))
    addr = block.start
    
    # check if first instruction is mov w0, #0
    dis = bv.get_disassembly(addr)
    if "mov" in dis and "#0x1" in dis:
        return True
    elif "mov" in dis and "#0" in dis:
        return False
    else:
        return None
    
    
### Check availability of features (not yet if they are true)
def check_ids_strings():
    load_framework("/System/Library/PrivateFrameworks/IDSFoundation.framework/IDSFoundation")
    prefix = "_IDSRegistrationProperty"
    for sym_list in bv.symbols.values():
        for sym in sym_list:
            if sym.name.startswith(prefix):
                cfstr_str = read_nsstring(read_ptr(sym.address))
                capabilities[cfstr_str] = "available"

def check_ids_properties():
    if "kt-version" in capabilities:
        capabilities["kt-version"] = read_nsint_return("__IDSKeyTransparencyVersionNumber")

    if  "ec-version" in capabilities:
        capabilities["ec-version"] = read_nsint_return("__IDSECVersion")

def check_devicesupport_properties():
    load_framework("/System/Library/PrivateFrameworks/FTServices.framework/FTServices")

    # contains values that are hardcoded - mostly on macOS, in iOS hardware is actually checked

    if check_return_bool("-[FTDeviceSupport supportsUWB]") == False:
        capabilities["supports-uwb"] = False

    if check_return_bool("-[FTDeviceSupport supportsHarmony]") == False:
        capabilities["supports-harmony"] = False

    if check_return_bool("-[FTDeviceSupport supportsFMDV2]") == True:
        capabilities["supports-fmd-v2"] = True

    if check_return_bool("-[FTDeviceSupport supportsAnimojiV2]") == True:
        capabilities["supports-animoji-v2"] = True

    if check_return_bool("-[FTDeviceSupport supportsHEIFEncoding]") == True:
        capabilities["supports-heif"] = True

    if check_return_bool("-[FTDeviceSupport supportsHDRdecoding]") == True:
        capabilities["supports-hdr"] = True

    if check_return_bool("-[FTDeviceSupport isC2KEquipment]") == False:
        capabilities["is-c2k-equipment"] = False

    if check_return_bool("-[FTDeviceSupport supportsStewie]") == False:
        capabilities["supports-stewie"] = False
    


### Zelkova, not available on iOS 16.7.12
def check_zelkova():
    # leave empty and don't try to decode if it's not available
    if not "supports-zelkova" in capabilities:
        return
    
    load_framework("/System/Library/PrivateFrameworks/SafetyMonitor.framework/SafetyMonitor")
    syms = bv.get_symbols_by_name("_isEligibleForReceivingZelkova")

    capabilities["supports-zelkova"] = False
    if not syms:
        return

    # cannot use check_return_bool here because it's not the first mov instruction and then a straight return but some logging on top.
    # so we have to parse the whole function and see what it returns.
    f = bv.get_function_at(syms[0].address)
    mlil = f.mlil
    for insn in f.mlil.instructions:
        if insn.operation == M.MLIL_RET:
            for i, expr in enumerate(insn.src):
                pv = expr.value
                if pv.value == 1:
                    capabilities["supports-zelkova"] = True


### DeviceTree Checks
def check_devicetree() -> any:
    
    subprocess.run(
        ["ipsw", "extract", "--dtree", "-o", str(OUT), str(IPSW)],
        check=True,
    )

    dtree_files = list(OUT.glob("*/Firmware/all_flash/DeviceTree*im4p"))
    if not dtree_files:
        raise SystemExit(f"No DeviceTree*.im4p found under {OUT}")

    dtree_path = dtree_files[0]

    res = subprocess.run(
        ["ipsw", "dtree", "-j", str(dtree_path)],
        check=True,
        capture_output=True,
        text=True,
    )

    devicetree_json = OUT / "devicetree.json"
    devicetree_json.write_text(res.stdout, encoding="utf-8")

    return devicetree_json.read_text(encoding="utf-8", errors="replace")

def check_dt_uwb(dt):
    if not "supports-uwb" in capabilities:
        return

    if '{"rose":' in dt:
        capabilities["supports-uwb"] = True
    else:
        capabilities["supports-uwb"] = False

def check_dt_heif_parse(dt) -> bool:
    TARGET_KEYS = {"graphics-featureset-class", "graphics-featureset-fallbacks"}
    TARGET_TOKENS = {"APPLE2", "MTL2"}

    if isinstance(dt, (str, bytes, bytearray)):
        dt = json.loads(dt)

    stack = [dt]

    while stack:
        cur = stack.pop()

        if isinstance(cur, dict):
            for k, v in cur.items():
                if k in TARGET_KEYS:
                    # Normalize value to string if possible
                    if isinstance(v, (bytes, bytearray)):
                        v = v.decode("utf-8", errors="ignore")

                    if isinstance(v, str):
                        # token-aware match: split on ':' and ',' so "GLES2,0" is handled
                        tokens = v.replace(",", ":").split(":")
                        if any(t in TARGET_TOKENS for t in tokens):
                            return True

                # keep walking
                stack.append(v)

        elif isinstance(cur, list):
            stack.extend(cur)

    return False

def check_dt_heif(dt):
    if not "supports-heif" in capabilities:
        return
    
    capabilities["supports-heif"] = check_dt_heif_parse(dt)

def check_dt_hevc(dt):
    # default: true
    # only exceptions:
    # - H9 SoC Generation (devie tree soc-generation) and platform-name != s8001
    # - M9 or H10 SoC generation (device tree soc-generation) -> on iPhone that is false and then it returns true
    # - device type 7 (homepod?) is always false

    if not "supports-hdr" in capabilities:
        return

    if '"soc-generation":"M9"' in dt:
        capabilities["supports-hdr"] = False

    if '"soc-generation":"H9"' in dt:
        capabilities["supports-hdr"] = False

    if '"soc-generation":"H10"' in dt:
        capabilities["supports-hdr"] = False



### Main logic that calls all functions above

dyld = extract_dyld_shared_cache_from_ipsw(IPSW)
if dyld:
    bv = bn.load(dyld)
    dsc = sharedcache.sharedcache.SharedCacheController(bv)
    check_ids_strings()
    check_ids_properties()
    check_devicesupport_properties()
    check_zelkova()

dt = check_devicetree()
check_dt_uwb(dt)
check_dt_heif(dt)
check_dt_hevc(dt)


### Print results
print(json.dumps(capabilities, indent=2))

out_file = RESULTS / "capabilities.json"

with out_file.open("w", encoding="utf-8") as f:
    json.dump(capabilities, f, indent=2)


### Cleanup
if OUT.exists() and OUT.is_dir():
    shutil.rmtree(OUT)

dsc = None
bv = None