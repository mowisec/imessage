#!/usr/bin/env python3

import serial
import sys
import threading
import argparse
import time
from datetime import datetime, timezone
from pathlib import Path

LOG_DIR = "logs"
DEFAULT_BAUDRATE = 115200
SERIAL_TIMEOUT_SECONDS = 0.1
RECONNECT_DELAY_SECONDS = 1.0


def utc_timestamp():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def reader(ser, logfile, sleeptime_s, stop_event):
    while not stop_event.is_set():
        try:
            data = ser.readline()
            if not data:
                time.sleep(sleeptime_s)
                continue

            text = data.decode(errors="replace").rstrip("\n")
            line = f"[{utc_timestamp()}] {text}"

            print(line)
            logfile.write(line + "\n")
            logfile.flush()

        except Exception as e:
            print(f"[WARN] Serial read error: {e}", file=sys.stderr)
            stop_event.set()
            break


def writer(ser, stop_event):
    while not stop_event.is_set():
        try:
            line = sys.stdin.readline()
            if not line:
                stop_event.set()
                break
            ser.write(line.encode())
        except Exception as e:
            print(f"[WARN] Serial write error: {e}", file=sys.stderr)
            stop_event.set()
            break


def parse_args():
    parser = argparse.ArgumentParser(
        description="Resilient serial TTY logger with UTC timestamps"
    )
    parser.add_argument(
        "--device",
        default="/dev/ttyUSB0",
        help="Serial device path (default: /dev/ttyUSB0)",
    )
    parser.add_argument(
        "--sleeptime",
        type=int,
        default=10,
        help="Sleep time in milliseconds when no data is received (default: 10)",
    )
    parser.add_argument(
        "--baudrate",
        type=int,
        default=DEFAULT_BAUDRATE,
        help=f"Serial baudrate (default: {DEFAULT_BAUDRATE})",
    )
    return parser.parse_args()


def main():
    args = parse_args()
    sleeptime_s = args.sleeptime / 1000.0

    Path(LOG_DIR).mkdir(exist_ok=True)
    logfile_name = f"serial_{datetime.now(timezone.utc).strftime('%Y%m%d_%H%M%S')}.log"
    logfile_path = Path(LOG_DIR) / logfile_name

    print(f"Device    : {args.device}")
    print(f"Baudrate  : {args.baudrate}")
    print(f"Sleeptime : {args.sleeptime} ms")
    print(f"Logging to {logfile_path}")
    print("Waiting for device... (Ctrl+C to exit)\n")

    with open(logfile_path, "a", buffering=1) as logfile:
        while True:
            try:
                ser = serial.Serial(
                    args.device,
                    args.baudrate,
                    timeout=SERIAL_TIMEOUT_SECONDS,
                )
                print(f"[INFO] Connected to {args.device}")

                stop_event = threading.Event()

                t_reader = threading.Thread(
                    target=reader,
                    args=(ser, logfile, sleeptime_s, stop_event),
                    daemon=True,
                )
                t_writer = threading.Thread(
                    target=writer,
                    args=(ser, stop_event),
                    daemon=True,
                )

                t_reader.start()
                t_writer.start()

                # Wait until one thread signals failure
                while not stop_event.is_set():
                    time.sleep(0.1)

            except serial.SerialException as e:
                print(f"[WARN] Serial open failed: {e}", file=sys.stderr)

            except KeyboardInterrupt:
                print("\nExiting...")
                break

            finally:
                try:
                    ser.close()
                except Exception:
                    pass

                print(
                    f"[INFO] Disconnected. Reconnecting in {RECONNECT_DELAY_SECONDS}s...\n"
                )
                time.sleep(RECONNECT_DELAY_SECONDS)


if __name__ == "__main__":
    main()
