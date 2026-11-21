#!/usr/bin/env python3

import argparse
import re
import shlex
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from pathlib import Path

CLI_DESCRIPTION = """Run a winedroid automation journey on Android through adb.

The command executes the WINEDROID_AUTOMATION_SCRIPT format against a real
Android device so one journey can drive both backends. Supported verbs are tap
(point, text, id), swipe, key, text, waitidle, waittext, assert
text/notext/view, screenshot, and dump ui. Relative screenshot and dump paths
resolve inside --artifacts.

python3 scripts/emu_journey.py --artifacts out/emulator journey.auto
"""

KEY_EVENTS = {
    "back": "KEYCODE_BACK",
    "home": "KEYCODE_HOME",
    "menu": "KEYCODE_MENU",
    "enter": "KEYCODE_ENTER",
    "del": "KEYCODE_DEL",
    "tab": "KEYCODE_TAB",
    "space": "KEYCODE_SPACE",
    "volup": "KEYCODE_VOLUME_UP",
    "voldown": "KEYCODE_VOLUME_DOWN",
}

DEFAULT_WAIT_TIMEOUT_MS = 5000


class JourneyError(Exception):
    pass


class Adb:
    def __init__(self, serial):
        self.serial = serial

    def run(self, *args, binary=False, check=True):
        command = ["adb"]
        if self.serial:
            command += ["-s", self.serial]
        command += list(args)
        result = subprocess.run(command, capture_output=True)
        if check and result.returncode != 0:
            raise JourneyError(
                f"`{' '.join(command)}` failed: {result.stderr.decode(errors='replace').strip()}"
            )
        return result.stdout if binary else result.stdout.decode(errors="replace")

    def shell(self, *args, check=True):
        return self.run("shell", *args, check=check)


def parse_line(line_number, raw):
    line = raw.split("#", 1)[0].strip()
    if not line:
        return None
    try:
        tokens = shlex.split(line)
    except ValueError as error:
        raise JourneyError(f"line {line_number}: {error}")
    if len(tokens) < 2:
        raise JourneyError(f"line {line_number}: expected `<delay_ms> <verb> [args]`")
    if not tokens[0].isdigit():
        raise JourneyError(f"line {line_number}: delay must be a non-negative integer")
    return int(tokens[0]), tokens[1], tokens[2:], line_number


def parse_coord(token, line_number):
    match = re.fullmatch(r"(\d+):(\d+)", token)
    if not match:
        raise JourneyError(f"line {line_number}: coordinate must be `X:Y`, got `{token}`")
    return int(match.group(1)), int(match.group(2))


class Driver:
    def __init__(self, adb, artifacts):
        self.adb = adb
        self.artifacts = artifacts

    def dump_tree(self):
        self.adb.shell("uiautomator", "dump", "/sdcard/.journey-dump.xml")
        raw = self.adb.run("exec-out", "cat", "/sdcard/.journey-dump.xml", binary=True)
        try:
            return ET.fromstring(raw), raw
        except ET.ParseError as error:
            raise JourneyError(f"uiautomator dump did not parse: {error}")

    def nodes(self, tree):
        return list(tree.iter("node"))

    def node_center(self, node):
        match = re.fullmatch(r"\[(\d+),(\d+)\]\[(\d+),(\d+)\]", node.get("bounds", ""))
        if not match:
            raise JourneyError(f"node has unparseable bounds `{node.get('bounds')}`")
        left, top, right, bottom = (int(g) for g in match.groups())
        return (left + right) // 2, (top + bottom) // 2

    def find_text(self, tree, text):
        matches = [n for n in self.nodes(tree) if n.get("text") == text]
        return matches[-1] if matches else None

    def find_id(self, tree, view_id):
        if view_id.startswith("0x") or view_id.isdigit():
            raise JourneyError(
                f"tap id `{view_id}`: numeric view ids cannot resolve on the emulator, "
                "use `name` or `package:id/name`"
            )
        expected_exact = view_id if ":id/" in view_id else None
        expected_suffix = None if expected_exact else f":id/{view_id}"
        matches = [
            n
            for n in self.nodes(tree)
            if (expected_exact and n.get("resource-id") == expected_exact)
            or (expected_suffix and n.get("resource-id", "").endswith(expected_suffix))
        ]
        return matches[-1] if matches else None

    def contains_text(self, tree, text):
        return any(
            text in n.get("text", "") or text in n.get("content-desc", "")
            for n in self.nodes(tree)
        )

    def tap_at(self, x, y, hold_ms):
        if hold_ms > 0:
            self.adb.shell("input", "swipe", str(x), str(y), str(x), str(y), str(hold_ms))
        else:
            self.adb.shell("input", "tap", str(x), str(y))

    def artifact_path(self, token):
        path = Path(token)
        if not path.is_absolute():
            path = self.artifacts / path
        path.parent.mkdir(parents=True, exist_ok=True)
        return path

    def execute(self, delay_ms, verb, args, line_number):
        time.sleep(delay_ms / 1000.0)

        if verb == "tap":
            if args and args[0] == "text":
                target = lambda tree: self.find_text(tree, args[1])
                description = f"text {args[1]!r}"
                hold_ms = int(args[2]) if len(args) > 2 else 0
            elif args and args[0] == "id":
                target = lambda tree: self.find_id(tree, args[1])
                description = f"id `{args[1]}`"
                hold_ms = int(args[2]) if len(args) > 2 else 0
            else:
                x, y = parse_coord(args[0], line_number)
                self.tap_at(x, y, int(args[1]) if len(args) > 1 else 0)
                return
            tree, _ = self.dump_tree()
            node = target(tree)
            if node is None:
                raise JourneyError(f"line {line_number}: tap {description} matched no node")
            x, y = self.node_center(node)
            self.tap_at(x, y, hold_ms)
        elif verb == "swipe":
            start = parse_coord(args[0], line_number)
            end = parse_coord(args[1], line_number)
            duration_ms = args[2] if len(args) > 2 else "300"
            self.adb.shell(
                "input", "swipe",
                str(start[0]), str(start[1]), str(end[0]), str(end[1]), duration_ms,
            )
        elif verb == "key":
            key = KEY_EVENTS.get(args[0])
            if key is None:
                raise JourneyError(f"line {line_number}: unknown key `{args[0]}`")
            self.adb.shell("input", "keyevent", key)
        elif verb == "text":
            self.adb.shell("input", "text", args[0].replace(" ", "%s"))
        elif verb == "waitidle":
            timeout_ms = int(args[0]) if args else DEFAULT_WAIT_TIMEOUT_MS
            deadline = time.monotonic() + timeout_ms / 1000.0
            _, previous = self.dump_tree()
            while True:
                time.sleep(0.5)
                _, current = self.dump_tree()
                if current == previous:
                    return
                previous = current
                if time.monotonic() >= deadline:
                    raise JourneyError(
                        f"line {line_number}: waitidle timed out before the UI settled"
                    )
        elif verb == "waittext":
            timeout_ms = int(args[1]) if len(args) > 1 else DEFAULT_WAIT_TIMEOUT_MS
            deadline = time.monotonic() + timeout_ms / 1000.0
            while True:
                tree, _ = self.dump_tree()
                if self.contains_text(tree, args[0]):
                    return
                if time.monotonic() >= deadline:
                    raise JourneyError(
                        f"line {line_number}: waittext `{args[0]}` timed out"
                    )
                time.sleep(0.5)
        elif verb == "assert":
            tree, _ = self.dump_tree()
            if args[0] == "text":
                if not self.contains_text(tree, args[1]):
                    raise JourneyError(f"line {line_number}: assert text `{args[1]}` failed")
            elif args[0] == "notext":
                if self.contains_text(tree, args[1]):
                    raise JourneyError(f"line {line_number}: assert notext `{args[1]}` failed")
            elif args[0] == "view":
                if self.find_id(tree, args[1]) is None:
                    raise JourneyError(f"line {line_number}: assert view `{args[1]}` failed")
            else:
                raise JourneyError(
                    f"line {line_number}: assert `{args[0]}` is not supported on the emulator"
                )
        elif verb == "screenshot":
            path = self.artifact_path(args[0] if args else "screenshot.png")
            path.write_bytes(self.adb.run("exec-out", "screencap", "-p", binary=True))
        elif verb == "dump":
            if not args or args[0] != "ui":
                raise JourneyError(
                    f"line {line_number}: only `dump ui [path]` is supported on the emulator"
                )
            _, raw = self.dump_tree()
            if len(args) > 1:
                self.artifact_path(args[1]).write_bytes(raw)
            else:
                sys.stdout.write(raw.decode(errors="replace") + "\n")
        elif verb in ("down", "move", "up", "scroll", "rotary"):
            raise JourneyError(
                f"line {line_number}: `{verb}` is not supported on the emulator driver yet"
            )
        else:
            raise JourneyError(f"line {line_number}: unknown verb `{verb}`")


def main():
    parser = argparse.ArgumentParser(
        description=CLI_DESCRIPTION,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("journey", type=Path)
    parser.add_argument("--serial", default=None)
    parser.add_argument("--artifacts", type=Path, default=Path("."))
    options = parser.parse_args()

    steps = []
    for line_number, raw in enumerate(options.journey.read_text().splitlines(), start=1):
        parsed = parse_line(line_number, raw)
        if parsed:
            steps.append(parsed)

    driver = Driver(Adb(options.serial), options.artifacts)
    for delay_ms, verb, args, line_number in steps:
        print(f"journey line {line_number}: {verb} {' '.join(args)}", flush=True)
        driver.execute(delay_ms, verb, args, line_number)
    print("journey: OK")


if __name__ == "__main__":
    try:
        main()
    except JourneyError as error:
        print(f"journey: FAILED — {error}", file=sys.stderr)
        sys.exit(1)
