#!/usr/bin/env python3
"""Sanity check of parity screenshots before they are compared.

usage: check_capture.py --side apple|ours --dir DIR --prefix PREFIX --cases CASES
Prints the cases (`kind:return:autocorrect`, comma-separated) whose screenshot
does not show the expected keyboard, nothing when all are fine; reasons go to
stderr. parity.sh captures those cases again.

A good capture has the four key rows of a keyboard (a full top row of letters
unless it is the digit pad) and the right keyboard in front: Apple's keyboard
lists its keys in the app's accessibility tree, ours runs in its own process
and lists none. When our extension does not come up in time, iOS shows its own
keyboard in its place, which only the accessibility tree tells apart.
"""
import argparse
import os
import sys

from compare import detect, load


def single_character_keys(ax_path):
    if not os.path.exists(ax_path):
        return 0
    count = 0
    with open(ax_path, encoding="utf-8") as f:
        for line in f:
            label = line.split("\t", 1)[0]
            if not line.startswith("#") and len(label) == 1:
                count += 1
    return count


def problem(side, kind, png_path):
    if not os.path.exists(png_path):
        return "no screenshot"
    found = detect(load(png_path))
    if found is None:
        return "no keyboard found"
    rows = found["rows"]
    if len(rows) != 4:
        return f"{len(rows)} key rows instead of 4"
    if kind != "number" and len(rows[0]) < 10:
        return f"top row has {len(rows[0])} keys"
    keys = single_character_keys(png_path[:-4] + ".ax.tsv")
    if side == "apple" and keys < 10:
        return f"Apple's keyboard lists {keys} keys in Accessibility"
    if side == "ours" and keys > 0:
        return f"the system keyboard is showing ({keys} keys in Accessibility), not ours"
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--side", choices=["apple", "ours"], required=True)
    parser.add_argument("--dir", required=True)
    parser.add_argument("--prefix", required=True)
    parser.add_argument("--cases", required=True)
    args = parser.parse_args()
    bad = []
    for spec in filter(None, args.cases.split(",")):
        kind, ret, autocorrect = spec.split(":")
        name = f"{args.prefix}-{kind}-{ret}-ac{autocorrect}"
        reason = problem(args.side, kind, os.path.join(args.dir, name + ".png"))
        if reason:
            print(f"parity: {name}: {reason}", file=sys.stderr)
            bad.append(spec)
    print(",".join(bad))


if __name__ == "__main__":
    main()
