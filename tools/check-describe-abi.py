#!/usr/bin/env python3
"""Check a describe module's contract: no imports at all, and exactly the
`alloc` and `describe` functions exported (crates/sdk/describe)."""
import pathlib
import re
import subprocess
import sys

expected_exports = {"alloc", "describe"}
failed = False
for argument in sys.argv[1:]:
    path = pathlib.Path(argument)
    result = subprocess.run(["wasm-tools", "print", str(path)], capture_output=True, text=True)
    if result.returncode:
        print(f"{path.name}: wasm-tools exit={result.returncode}: {result.stderr[:200]}")
        failed = True
        continue
    imports = re.findall(r'\(import "([^"]+)" "([^"]+)"', result.stdout)
    exports = set(re.findall(r'\(export "([^"]+)" \(func ', result.stdout))
    good = not imports and exports == expected_exports
    print(f"{path.name}: {'PASS' if good else 'FAIL'}; imports={len(imports)}, function_exports={sorted(exports)}")
    if not good:
        print(f"  imports: {imports[:5]}")
        failed = True
sys.exit(1 if failed else 0)
