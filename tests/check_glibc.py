#!/usr/bin/env python3
"""Reject release binaries requiring glibc newer than our oldest supported OS."""
import argparse
import os
import re
import subprocess
import sys


def version_tuple(version):
    parts = tuple(int(part) for part in version.split("."))
    return parts + (0,) * max(0, 3 - len(parts))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary")
    parser.add_argument("--max-version", default="2.35", help="Ubuntu 22.04 provides glibc 2.35")
    args = parser.parse_args()
    if not re.fullmatch(r"\d+(?:\.\d+)+", args.max_version):
        parser.error("--max-version must be a numeric version such as 2.35")
    result = subprocess.run(
        ["readelf", "--version-info", "--wide", args.binary],
        check=True, capture_output=True, text=True, env={**os.environ, "LC_ALL": "C"},
    )
    names = set(re.findall(r"\bName:\s+(GLIBC_\S+)", result.stdout))
    if not names:
        sys.exit("FAIL: no GLIBC version requirements found; expected a dynamically linked GNU/Linux binary")
    unsupported = sorted(name for name in names if not re.fullmatch(r"GLIBC_\d+(?:\.\d+)+", name))
    if unsupported:
        sys.exit("FAIL: unrecognized GLIBC requirements: " + ", ".join(unsupported))
    highest = max((name.removeprefix("GLIBC_") for name in names), key=version_tuple)
    if version_tuple(highest) > version_tuple(args.max_version):
        sys.exit(f"FAIL: {args.binary} requires glibc {highest}; maximum allowed is {args.max_version}")
    print(f"PASS: {args.binary} requires glibc {highest} <= {args.max_version}")


if __name__ == "__main__":
    main()
