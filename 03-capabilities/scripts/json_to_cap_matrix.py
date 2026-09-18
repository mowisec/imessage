#!/usr/bin/env python3
import argparse
import csv
import json
from pathlib import Path
from typing import Any, Dict, List, Tuple

ABSENT = "<absent>"

def normalize_cell(value: str) -> str:
    v = value.strip()
    if v.lower() == "true":
        return "T"
    elif v.lower() == "false":
        return "F"
    elif v == ABSENT:
        return "n/a"
    return v

def iter_devices(item: Dict[str, Any]) -> List[Tuple[str, int]]:
    # examples: [{"handle":[...], "idx":[...]}]
    out: List[Tuple[str, int]] = []
    for ex in item.get("examples", []):
        handles = ex.get("handle", [])
        idxs = ex.get("idx", [])
        for h in handles:
            for i in idxs:
                out.append((str(h), int(i)))
    return out

def build_table(data: List[Dict[str, Any]]) -> Tuple[List[str], List[List[str]]]:
    # Collect all devices
    device_keys_set = set()
    for cap in data:
        for vobj in cap.get("values", []):
            for h, i in iter_devices(vobj):
                device_keys_set.add(f"{h}#{i}")

    device_keys = sorted(device_keys_set)

    # capability -> device -> cell
    table: List[List[str]] = []
    for cap in data:
        name = cap.get("name", "")
        row_map: Dict[str, str] = {dk: "" for dk in device_keys}

        for vobj in cap.get("values", []):
            raw_val = str(vobj.get("value", ""))
            cell_val = normalize_cell(raw_val)
            for h, i in iter_devices(vobj):
                dk = f"{h}#{i}"
                if dk not in row_map:
                    continue
                prev = row_map[dk]
                if prev in ("", "n/a"):
                    row_map[dk] = cell_val
                else:
                    # keep first non-empty; flag conflicts if new differs and is non-empty
                    if cell_val != "" and cell_val != prev:
                        row_map[dk] = f"CONFLICT({prev}|{cell_val})"

        table.append([name] + [row_map[dk] for dk in device_keys])

    header = ["Capability"] + device_keys
    return header, table

def main() -> None:
    ap = argparse.ArgumentParser(description="Convert capability JSON to device-matrix CSV.")
    ap.add_argument("input_json", type=Path, help="Path to input JSON file")
    ap.add_argument("output_csv", type=Path, help="Path to output CSV file")
    args = ap.parse_args()

    data = json.loads(args.input_json.read_text(encoding="utf-8"))
    if not isinstance(data, list):
        raise SystemExit("Input JSON must be a list at the top level.")

    header, rows = build_table(data)

    args.output_csv.parent.mkdir(parents=True, exist_ok=True)
    with args.output_csv.open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(header)
        w.writerows(rows)

if __name__ == "__main__":
    main()
