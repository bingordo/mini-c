#!/usr/bin/env python3
"""Generated x86-64 C99 bit-field ABI byte-image differentials against GCC.

Compares object sizes, ordinary-member offsets, enclosing struct alignment,
static bytes, and automatic-initializer/mutation bytes. Padding byte images
are GCC-specific empirical ABI observations, not ISO C portability guarantees.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


@dataclass(frozen=True)
class Case:
    name: str
    members: str
    initializer: str
    mutation: str
    observed_member: str = "tail"
    kind: str = "struct"


def cases() -> list[Case]:
    result: list[Case] = []
    # Cover shared words, exact word ends, and fields that have to start a
    # new allocation unit rather than crossing their declared 32-bit unit.
    for first, second in [
        (1, 1), (3, 5), (7, 24), (7, 29), (10, 21), (13, 17),
        (24, 12), (30, 4), (31, 3), (32, 1), (1, 32), (29, 30),
    ]:
        a = (1 << first) - 1
        b = (1 << second) - 1
        result.append(Case(
            f"unsigned_{first}_{second}",
            f"unsigned int a:{first},b:{second},c:5;unsigned char tail;",
            f"{{.a={a}u,.b={b}u,.c=17,.tail=0xa5}}",
            f"local.a={a >> 1}u;local.b={b >> 1}u;local.c=11;",
        ))
    for width in [2, 3, 5, 7, 16, 31, 32]:
        value = -(1 << (width - 2))
        result.append(Case(
            f"signed_{width}",
            f"signed int a:{width};unsigned int b:7;unsigned char tail;",
            f"{{.a={value},.b=81,.tail=0x5a}}",
            "local.a=-1;local.b=21;",
        ))
    result.extend([
        Case("int_after_char", "char prefix;unsigned int a:3,b:5;char tail;",
             "{'A',5,23,'Z'}", "local.a=3;local.b=7;"),
        Case("int_after_short", "short prefix;unsigned int a:3,b:5;char tail;",
             "{1234,5,23,'Z'}", "local.a=3;local.b=7;"),
        Case("unnamed_gap", "unsigned int a:3;unsigned int :2;unsigned int b:5;char tail;",
             "{5,23,'X'}", "local.a=1;local.b=18;"),
        Case("zero_width_32", "unsigned int a:3;unsigned int :0;unsigned int b:5;char tail;",
             "{5,23,'X'}", "local.a=1;local.b=18;"),
        Case("zero_width_after_char", "char prefix;unsigned int :0;char tail;",
             "{'A','B'}", "local.prefix='Q';local.tail='R';"),
        Case("unnamed_after_char", "char prefix;unsigned int :3;char tail;",
             "{'A','B'}", "local.prefix='Q';local.tail='R';"),
        Case("bool_packed_byte", "_Bool a:1,b:1;char tail;",
             "{1,1,'B'}", "local.a=0;local.b=255;"),
        Case("bool_then_int", "_Bool a:1;unsigned int b:3;_Bool c:1;char tail;",
             "{1,5,1,'B'}", "local.a=0;local.b=7;local.c=0;"),
        Case("bool_after_nine_bits", "unsigned int a:9;_Bool b:1;unsigned int c:5;char tail;",
             "{511,1,29,'B'}", "local.a=257;local.b=0;local.c=17;"),
        Case("bool_unnamed", "_Bool a:1;_Bool :1;_Bool b:1;char tail;",
             "{1,1,'B'}", "local.a=0;local.b=0;"),
        Case("bool_zero_width", "char prefix;_Bool :0;_Bool a:1;char tail;",
             "{'A',1,'B'}", "local.a=0;local.tail='C';"),
        Case("mixed_zero_width", "_Bool a:1;unsigned int :0;_Bool b:1;char tail;",
             "{1,1,'B'}", "local.a=0;local.b=0;"),
        Case("union_signed", "signed int a:5;long wide;char tail[8];",
             "{.a=-7}", "local.a=-9;", kind="union"),
        Case("union_bool", "_Bool a:1;long wide;char tail[8];",
             "{.a=1}", "local.a=0;", kind="union"),
    ])
    return result


def source_for(case: Case) -> str:
    # External write() avoids libc headers and keeps the probe valid C99.
    # A wrapper's ordinary-member offset gives the aggregate's alignment,
    # without C11 _Alignof or taking the address of a bit-field.
    return f"""
        extern long write(int, const void *, unsigned long);
        {case.kind} S {{{case.members}}};
        struct Wrapper {{char lead;{case.kind} S nested;}};
        {case.kind} S global={case.initializer};
        int main(void) {{
            {case.kind} S local={case.initializer};
            struct Wrapper wrapper;
            unsigned long meta[4];
            meta[0]=sizeof(global);
            meta[1]=(unsigned long)((char*)&global.{case.observed_member}-(char*)&global);
            meta[2]=(unsigned long)((char*)&wrapper.nested-(char*)&wrapper);
            meta[3]=sizeof(wrapper);
            write(1,meta,sizeof(meta));
            write(1,&global,sizeof(global));
            write(1,&local,sizeof(local));
            {case.mutation}
            write(1,&local,sizeof(local));
            return 0;
        }}
    """ + "\n"


def run(command: list[str]) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(command, capture_output=True, timeout=90, check=False)


def checked(case: Case, phase: str, command: list[str]) -> bytes:
    result = run(command)
    if result.returncode or result.stderr:
        raise AssertionError(
            f"{case.name} {phase}: {' '.join(command)} exit={result.returncode}; "
            f"stdout={result.stdout[:300]!r}; stderr={result.stderr[:1000]!r}")
    return result.stdout


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path, default=ROOT / "target/debug/mini-c-cranelift")
    parser.add_argument("--gcc-only", action="store_true",
                        help="validate generated strict C99 cases without Mini-C")
    args = parser.parse_args()
    backend = args.backend.resolve()
    matrix = cases()
    if len({case.name for case in matrix}) != len(matrix):
        raise AssertionError("duplicate generated ABI case names")
    with tempfile.TemporaryDirectory(prefix="mini-c-bitfield-abi-") as temp:
        base = Path(temp)
        for case in matrix:
            src = base / f"{case.name}.c"
            gcc = base / f"{case.name}.gcc"
            obj = base / f"{case.name}.o"
            mini = base / f"{case.name}.mini"
            src.write_text(source_for(case), encoding="utf-8")
            checked(case, "GCC build",
                    ["cc", "-std=c99", "-pedantic-errors", "-O0", str(src), "-o", str(gcc)])
            gcc_bytes = checked(case, "GCC execute", [str(gcc)])
            if len(gcc_bytes) < 32:
                raise AssertionError(f"{case.name}: missing ABI metadata")
            # Only compare defined size/ordinary offsets with Clang;
            # C padding bytes are implementation-dependent.
            if shutil.which("clang"):
                clang_bin = base / f"{case.name}.clang"
                checked(case, "Clang build",
                        ["clang", "-std=c99", "-pedantic-errors", "-O0", str(src), "-o", str(clang_bin)])
                clang_bytes = checked(case, "Clang execute", [str(clang_bin)])
                if gcc_bytes[:32] != clang_bytes[:32]:
                    raise AssertionError(
                        f"{case.name}: GCC/Clang ABI layout mismatch "
                        f"{gcc_bytes[:32].hex()} != {clang_bytes[:32].hex()}")
            if args.gcc_only:
                continue
            checked(case, "Mini-C object",
                    [str(backend), "--emit-obj", str(obj), str(src)])
            if not obj.read_bytes().startswith(b"\x7fELF"):
                raise AssertionError(f"{case.name}: missing native ELF magic")
            checked(case, "Mini-C link", ["cc", str(obj), "-o", str(mini)])
            mini_bytes = checked(case, "Mini-C execute", [str(mini)])
            if gcc_bytes != mini_bytes:
                index = next((i for i, pair in enumerate(zip(gcc_bytes, mini_bytes))
                              if pair[0] != pair[1]), min(len(gcc_bytes), len(mini_bytes)))
                raise AssertionError(
                    f"{case.name}: GCC/Mini-C byte mismatch at {index}: "
                    f"GCC={gcc_bytes.hex()} Mini-C={mini_bytes.hex()}\n"
                    f"Source:\n{source_for(case)}")
            print(f"{case.name}: GCC/Mini-C byte image/layout PASS")
    print(f"Bit-field ABI byte differential: {len(matrix)} generated cases "
          f"{'GCC/Clang layout PASS' if args.gcc_only else 'GCC/Mini-C PASS'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
