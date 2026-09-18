# 04 — ESP32 Automation and Server-Location Evaluation

To measure iMessage round-trip times under *known* device states, we drive the victim device through a repeatable cycle of screen-off, screen-on and app-in-foreground phases with an ESP32 that emulates a Bluetooth keyboard and injects the corresponding keystrokes. This folder contains that automation (the ESP32 firmware plus a timestamping serial logger that yields the ground-truth state timeline for Figures 6 and 7).
