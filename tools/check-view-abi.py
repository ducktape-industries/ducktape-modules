#!/usr/bin/env python3
"""Check the exact bare-WASM view import and function-export contract."""
import pathlib
import re
import subprocess
import sys

expected_exports = {"alloc", "init", "tick", "snapshot", "restore"}
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
    good = imports == [("ducktape_view", "panicked")] and exports == expected_exports
    print(f"{path.name}: {'PASS' if good else 'FAIL'}; imports={len(imports)}, function_exports={len(exports)}")
    if not good:
        print(f"  imports: {imports}")
        print(f"  first extra exports: {sorted(exports - expected_exports)[:5]}")
        print(f"  missing exports: {sorted(expected_exports - exports)}")
        failed = True
sys.exit(1 if failed else 0)
