#!/usr/bin/env python3
"""Original C99 pointer subtraction programs: GCC/Mini-C native differential.

Only defined pointer subtraction within one array (or its one-past pointer)
is executed. Compare compile, link and exit status, not just object emission.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

CASES = {
    "positive_negative_int_elements": r"""
        int main(void) {
            int values[8];
            return (&values[6] - &values[2]) != 4
                || (&values[2] - &values[6]) != -4
                || (&values[0] - &values[0]) != 0;
        }
    """,
    "one_past_and_char_bytes": r"""
        int main(void) {
            char text[13];
            char *after = text + 13;
            return after - text != 13 || text - after != -13;
        }
    """,
    "struct_element_stride": r"""
        struct Pair { char small; long large; };
        int main(void) {
            struct Pair rows[5];
            return (&rows[4] - &rows[1]) != 3
                || (&rows[0] - &rows[4]) != -4
                || sizeof(struct Pair) != 16;
        }
    """,
    "const_and_nonconst_pointers": r"""
        long diff(int *p, const int *q) { return p-q; }
        int main(void) {
            int values[9];
            return diff(values + 8, values + 2) != 6;
        }
    """,
    "signed_64_bit_ptrdiff": r"""
        typedef long ptrdiff_t;
        ptrdiff_t diff(short *p, short *q) { return p-q; }
        int main(void) {
            short values[15];
            ptrdiff_t d = diff(&values[0], &values[11]);
            return sizeof(d) != 8 || d != -11;
        }
    """,
    "different_subscript_side_effects": r"""
        int main(void) {
            int values[10], left=2, right=7;
            long distance = &values[right++] - &values[left++];
            return distance != 5 || left != 3 || right != 8;
        }
    """,
}


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, text=True, capture_output=True,
                          check=False, timeout=45)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path,
                        default=ROOT / "target/debug/mini-c-cranelift")
    args = parser.parse_args()
    backend = args.backend.resolve()
    with tempfile.TemporaryDirectory(prefix="mini-c-ptrdiff-") as directory:
        base = Path(directory)
        for name, source in CASES.items():
            file = base / f"{name}.c"
            gcc = base / f"{name}.gcc"
            obj = base / f"{name}.o"
            mini = base / f"{name}.mini"
            file.write_text(source, encoding="utf-8")
            for command in (
                ["cc", "-std=c99", "-pedantic-errors", "-O0", str(file), "-o", str(gcc)],
                [str(backend), "--emit-obj", str(obj), str(file)],
                ["cc", str(obj), "-o", str(mini)],
                [str(gcc)],
                [str(mini)],
            ):
                completed = run(command)
                if completed.returncode != 0 or completed.stdout or completed.stderr:
                    raise AssertionError(
                        f"{name}: {' '.join(command)} failed: "
                        f"code={completed.returncode}, stdout={completed.stdout!r}, "
                        f"stderr={completed.stderr!r}"
                    )
            if not obj.read_bytes().startswith(b"\x7fELF"):
                raise AssertionError(f"{name}: expected native ELF object")
            print(f"{name}: GCC/Mini-C native ptrdiff execution PASS")
    print(f"C99 native pointer subtraction: {len(CASES)} cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
