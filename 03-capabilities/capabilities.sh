#!/bin/bash

IPSW="/iPhone17,3_26.2_23C5033h_Restore.ipsw"


OUT="/tmp/extracted/$(basename $IPSW)/"

# supports-uwb
# Maps down to having a `rose` device in the DeviceTree.
ipsw extract --dtree -o "$OUT" "$IPSW"
ipsw dtree -j $OUT/*/Firmware/all_flash/DeviceTree*im4p > $OUT/devicetree.json

if grep -q '{"rose":' $OUT/devicetree.json; then
    echo 'supports-uwb: 1'
else
    echo 'supports-uwb: 0'
fi
