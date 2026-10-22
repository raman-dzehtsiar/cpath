# Copyright (c) 2026 Raman Dzehtsiar
# SPDX-License-Identifier: MIT
"""Verify upstream provenance and reproducible C# parser generation.

Usage: python3 tools/check_csharp_grammar.py /path/to/tree-sitter
Requires Tree-sitter CLI 0.26.5, Node.js and Git, for maintainers only.
Regenerates in a temporary directory; never modifies the vendored parser.
Run the grammar corpus separately as documented in vendor/tree-sitter-c-sharp/CPATH.md.
"""

import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    executable = shutil.which(sys.argv[1])
    if not executable:
        raise SystemExit("Tree-sitter CLI executable not found")
    executable = str(Path(executable).resolve())
    version = subprocess.check_output([executable, "--version"], text=True).split()
    if len(version) < 2 or version[:2] != ["tree-sitter", "0.26.5"]:
        raise SystemExit("Use exactly Tree-sitter CLI 0.26.5")

    vendor = Path(__file__).resolve().parents[1] / "vendor" / "tree-sitter-c-sharp"
    upstream = json.loads((vendor / "UPSTREAM.json").read_text())
    generated = {
        "src/parser.c", "src/grammar.json", "src/node-types.json",
        "src/tree_sitter/alloc.h", "src/tree_sitter/array.h", "src/tree_sitter/parser.h",
    }
    patched_sources = {"grammar.js", "src/scanner.c"}
    intentional_changes = generated | patched_sources
    for name, digest in upstream["files"].items():
        if name not in intentional_changes:
            actual = hashlib.sha256((vendor / name).read_bytes()).hexdigest()
            if actual != digest:
                raise SystemExit(f"Unexpected upstream modification: {name}")

    with tempfile.TemporaryDirectory(prefix="cpath-grammar-") as temporary:
        work = Path(temporary)
        for name in sorted(patched_sources | {"tree-sitter.json", "package.json"}):
            (work / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(vendor / name, work / name)
        # Prove the reviewable patch reconstructs the recorded pristine source.
        # Apply only to the disposable copy, never to the working grammar.
        subprocess.run(
            ["git", "apply", "--reverse", "--whitespace=error", str(vendor / "cpath.patch")],
            cwd=work, check=True,
        )
        for name in sorted(patched_sources):
            original = hashlib.sha256((work / name).read_bytes()).hexdigest()
            if original != upstream["files"][name]:
                raise SystemExit(f"Patch does not reconstruct pinned upstream source: {name}")
            shutil.copyfile(vendor / name, work / name)
        subprocess.run([executable, "generate", "--abi", "15"], cwd=work, check=True)
        for name in sorted(generated):
            if (work / name).read_bytes() != (vendor / name).read_bytes():
                raise SystemExit(f"Generated artifact differs: {name}")
    print("Upstream files verified; generated parser, metadata and headers reproduce exactly.")


if __name__ == "__main__":
    main()
