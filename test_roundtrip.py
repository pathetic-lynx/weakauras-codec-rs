#!/usr/bin/env python3
"""Find .txt files containing WA strings and verify JSON roundtrip integrity."""

import json
import os
import subprocess
import sys

WA_BIN = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "target", "release", "wa"
)
SEARCH_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")


def run_wa(args, stdin_data):
    result = subprocess.run(
        [WA_BIN, *args],
        input=stdin_data,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None, result.stderr.strip()
    return result.stdout.strip(), None


def normalize(obj):
    """Recursively sort dicts by key so ordering doesn't affect comparison."""
    if isinstance(obj, dict):
        return {k: normalize(v) for k, v in sorted(obj.items())}
    if isinstance(obj, list):
        return [normalize(v) for v in obj]
    return obj


def test_file(path):
    try:
        wa_string = open(path).read().strip()
    except (UnicodeDecodeError, OSError) as e:
        return None, str(e)
    if not wa_string.startswith("!"):
        return None, "not a WA string"

    json1, err = run_wa(["decode"], wa_string)
    if err:
        return False, f"decode failed: {err}"

    wa2, err = run_wa(["encode"], json1)
    if err:
        return False, f"encode failed: {err}"

    json2, err = run_wa(["decode"], wa2)
    if err:
        return False, f"second decode failed: {err}"

    if json1 == json2:
        return True, None

    try:
        obj1 = normalize(json.loads(json1))
        obj2 = normalize(json.loads(json2))
    except json.JSONDecodeError as e:
        return False, f"JSON parse error during comparison: {e}"

    if obj1 == obj2:
        return True, "OK (key order differs)"
    else:
        return False, "JSON mismatch (semantic)"


def main():
    search_dir = sys.argv[1] if len(sys.argv) > 1 else SEARCH_DIR

    txt_files = []
    for root, _, files in os.walk(search_dir):
        for f in sorted(files):
            if f.endswith(".txt"):
                txt_files.append(os.path.join(root, f))

    if not txt_files:
        print(f"No .txt files found in {search_dir}")
        return 1

    passed = 0
    failed = 0
    skipped = 0

    for path in sorted(txt_files):
        rel = os.path.relpath(path, search_dir)
        result, err = test_file(path)
        if result is None:
            skipped += 1
            print(f"  SKIP  {rel} ({err})")
        elif result:
            passed += 1
            note = f" ({err})" if err else ""
            print(f"  OK    {rel}{note}")
        else:
            failed += 1
            print(f"  FAIL  {rel} ({err})")

    print(f"\n{passed} passed, {failed} failed, {skipped} skipped")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
