#!/usr/bin/env python3
"""Compare Mini-C Cranelift-generated ELF objects with GCC reference executions.

Requires previously built binaries: target/debug/mini-c-cranelift and
target/debug/mini-c-fuzz. Build them once via Cargo; this script never reruns
Cargo or downloads a compiler. Failures preserve the C and command outputs.
"""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys

PROGRAMS = {
    # SysV variadic SSE calls must set AL to the count of XMM registers
    # occupied by both fixed and ellipsis arguments; GCC formats the values.
    "sysv_varargs_printf_double_one": """extern int printf(const char *, ...);
        int main(void) { printf("%.2f|%.1f\\n", 1.25, -2.5); return 0; }""",
    "sysv_varargs_printf_float_promoted": """extern int printf(const char *, ...);
        int main(void) { float x=2.25f; printf("%.2f\\n", x); return 0; }""",
    "sysv_varargs_printf_nine_doubles": """extern int printf(const char *, ...);
        int main(void) { printf("%.0f|%.0f|%.0f|%.0f|%.0f|%.0f|%.0f|%.0f|%.0f\\n",
            1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0); return 0; }""",
    "sysv_varargs_mixed_gp_fp": """extern int printf(const char *, ...);
        int main(void) { printf("%d|%.2f|%ld|%.1f|%s|%d\\n",
            -17, 1.25, 1234567890L, 3.5, "ok", 9); return 0; }""",
    "sysv_varargs_snprintf_mixed_fp": """extern int snprintf(char *, unsigned long, const char *, ...);
        int main(void) { char buf[80];
            int n=snprintf(buf,80,"%.1f/%d/%.1f",1.5,7,2.5);
            return n!=9 || buf[0]!='1' || buf[3]!='/' || buf[8]!='5'; }""",
    # SysV MEMORY-class by-value arguments and hidden structure returns.
    "sysv_memory_stack_arg_padded_twenty_bytes": """struct Odd { int a,b,c,d,e; };
        int sum(struct Odd x) { return x.a+x.b+x.c+x.d+x.e; }
        int main(void) { struct Odd x={1,2,3,4,5}; return sum(x)!=15; }""",
    "sysv_memory_sret_twenty_bytes": """struct Odd { int a,b,c,d,e; };
        struct Odd echo(struct Odd x) { x.e+=4; return x; }
        int main(void) { struct Odd x={1,2,3,4,5};
            x=echo(x); return x.a!=1 || x.e!=9; }""",
    "sysv_memory_large_struct_argument": """struct Large { long a,b,c; };
        long sum(struct Large x) { x.a+=4; return x.a+x.b+x.c; }
        int main(void) { struct Large x={2,3,5};
            return sum(x)!=14 || x.a!=2; }""",
    "sysv_memory_large_struct_return": """struct Large { long a,b,c; };
        struct Large make(long a,long b,long c) {
            struct Large x={a,b,c}; return x;
        } int main(void) { struct Large x=make(7,11,13);
            return x.a!=7 || x.b!=11 || x.c!=13; }""",
    "sysv_memory_large_struct_roundtrip": """struct Large { long a,b,c; };
        struct Large roundtrip(struct Large x) { x.c+=9; return x; }
        int main(void) { struct Large x={2,3,5};
            x=roundtrip(x); return x.a!=2 || x.b!=3 || x.c!=14; }""",
    "sysv_register_exhaustion_struct_arg": """struct Pair { long a,b; };
        long sum(long a,long b,long c,long d,long e,long f,
                 struct Pair p) { return a+b+c+d+e+f+p.a+p.b; }
        int main(void) { struct Pair p={7,11}; return sum(1,2,3,4,5,6,p)!=39; }""",
    "sysv_sret_consumes_gp_register": """struct Large { long a,b,c; };
        struct Pair { long a,b; };
        struct Large f(long a,long b,long c,long d,long e,struct Pair p) {
            struct Large x={a+p.a,b+p.b,c+d+e}; return x;
        } int main(void) { struct Pair p={7,11}; struct Large x=f(1,2,3,4,5,p);
            return x.a!=8 || x.b!=13 || x.c!=12; }""",
    "sysv_sse_one_float_struct": """struct F { float v; };
        struct F twice(struct F a) { a.v*=2.0f; return a; }
        int main(void) { struct F f; f.v=1.25f; f=twice(f); return f.v!=2.5f; }""",
    "sysv_sse_packed_two_floats": """struct F { float x,y; };
        struct F swap(struct F a) { float t=a.x; a.x=a.y; a.y=t; return a; }
        int main(void) { struct F f; f.x=1.25f; f.y=2.5f;
            f=swap(f); return f.x!=2.5f || f.y!=1.25f; }""",
    "sysv_sse_two_double_eightbytes": """struct D { double x,y; };
        struct D adjust(struct D a) { a.x+=2.0; a.y-=1.0; return a; }
        int main(void) { struct D d; d.x=1.5; d.y=5.0;
            d=adjust(d); return d.x!=3.5 || d.y!=4.0; }""",
    "sysv_mixed_integer_sse_aggregate": """struct M { long n; double d; };
        struct M adjust(struct M x) { x.n+=3; x.d+=1.0; return x; }
        int main(void) { struct M m; m.n=7; m.d=2.5;
            m=adjust(m); return m.n!=10 || m.d!=3.5; }""",
    # Independent native F32/F64 execution and SysV SSE scalar call gates.
    "sysv_scalar_float_return_and_call": """float twice(float x) { return x*2.0f; }
        int main(void) { return twice(3.25f)!=6.5f; }""",
    "sysv_scalar_double_return_and_call": """double f(double x,double y) { return x*y+0.5; }
        int main(void) { return f(2.5,3.0)!=8.0; }""",
    "sysv_scalar_fp_integer_register_mix": """double f(int x,double a,long y,float b) {
            return (double)x+a+(double)y+(double)b;
        } int main(void) { return f(3,2.5,7,1.5f)!=14.0; }""",
    "sysv_scalar_fp_conversions": """double f(double a) { return (double)(int)a + 0.5; }
        int main(void) { return f(4.75)!=4.5; }""",
    "sysv_scalar_fp_comparison_and_condition": """int f(double a,double b) {
            return a<b ? 1 : a>=b ? 2 : 3;
        } int main(void) { return f(2.5,3.0)!=1 || f(4.0,4.0)!=2; }""",
    "sysv_scalar_fp_compound_update": """int main(void) { float x=1.25f; x*=2.0f; x++;
            return x!=3.5f; }""",
    # Register-class SysV aggregate calls and returns. Mixed GCC object tests
    # below validate the ABI independently from Mini-C's own caller/callee.
    "sysv_aggregate_two_integer_eightbytes": """struct Pair { long a; long b; };
        struct Pair make(long a,long b) { struct Pair p={a,b}; return p; }
        long use(struct Pair p) { return p.a * 3 + p.b; }
        int main(void) { struct Pair p=make(5,11); return use(p)!=26; }""",
    "sysv_aggregate_narrow_tail": """struct Tiny { char a; short b; };
        struct Tiny copy(struct Tiny x) { return x; }
        int main(void) { struct Tiny t={7,23}; struct Tiny v=copy(t);
            return v.a!=7 || v.b!=23; }""",
    "sysv_aggregate_twelve_bytes": """struct Three { int a,b,c; };
        struct Three adjust(struct Three x) { x.a+=2; x.c+=5; return x; }
        int main(void) { struct Three x={3,7,11}; x=adjust(x);
            return x.a!=5 || x.b!=7 || x.c!=16; }""",
    "sysv_aggregate_register_boundary": """struct Pair { long a,b; };
        long use(long x,long y,long z,long w,struct Pair p) {
            return x+y+z+w+p.a+p.b;
        }
        int main(void) { struct Pair p={6,7}; return use(1,2,3,4,p)!=23; }""",
    # FreeBSD ffs uses a zero guard around GCC __builtin_ctz(mask).
    # Compare defined negative/nonzero inputs and zero through the guard.
    "s21_ctz_guarded_ffs": """int ffs_like(int mask) {
        return mask == 0 ? 0 : __builtin_ctz(mask) + 1;
    }
    int main(void) {
        return ffs_like(0) != 0 || ffs_like(1) != 1
            || ffs_like(8) != 4 || ffs_like(-1) != 1
            || ffs_like(-2147483647-1) != 32;
    }""",
    "s21_ctz_argument_evaluated_once": """int calls;
    int next(void) { calls++; return 16; }
    int main(void) {
        int bits = __builtin_ctz(next());
        return (bits == 4 && calls == 1) ? 0 : 1;
    }""",
    # S21: GCC/libiberty sources use register on scalar parameters.
    # It is an automatic object hint, never a different calling convention.
    "s21_register_parameter_local_and_for_index": """int ffs_style(register int n) {
        register int copy=n;
        for (register int i=0; i<3; i++) copy+=i;
        return copy;
    }
    int main(void) { return ffs_style(11)-14; }""",
    # GP-only variadic ABI: check actual libc formatting, not merely emitted
    # ELF symbols or return codes. GCC is the independently compiled oracle.
    "sysv_varargs_mixed_width_register_and_stack": """extern int printf(const char *, ...);
        int main(void) {
            printf("%d|%ld|%u|%s|%d|%ld|%u\\n",
                -7, 4294967301L, 4000000000U, "ok", -3, -9L, 65535U);
            return 0;
        }""",
    "sysv_varargs_promoted_char_and_ushort": """extern int printf(const char *, ...);
        int main(void) {
            char c=-3; unsigned short u=65000;
            printf("%d:%d\\n", c, u); return 0;
        }""",
    "sysv_varargs_multiple_callsite_thunks": """extern int printf(const char *, ...);
        int main(void) {
            printf("first:%d\\n", 12);
            printf("second:%ld|%s\\n", 1234567890123L, "ok");
            return 0;
        }""",
    "sysv_varargs_snprintf_buffer_and_result": """extern int snprintf(char *, unsigned long, const char *, ...);
        int main(void) {
            char buffer[32];
            int n=snprintf(buffer, 32, "%s=%d", "ab", -17);
            return n==6 && buffer[0]=='a' && buffer[5]=='7';
        }""",
    "sysv_varargs_snprintf_stack_args": """extern int snprintf(char *, unsigned long, const char *, ...);
        int main(void) {
            char buffer[128];
            snprintf(buffer, 128, "%d:%d:%d:%d:%d:%d:%d",
                1, 2, 3, 4, 5, 6, 7);
            return buffer[0]+buffer[12]-'1'-'7';
        }""",
    "sysv_varargs_unsigned_longlong_max": """extern int printf(const char *, ...);
        int main(void) {
            printf("%llu %ld\\n", 18446744073709551615ULL, -1L);
            return 0;
        }""",
    # Recent semantic/codegen changes must agree with GCC at execution,
    # not merely emit a syntactically valid ELF object.
    "const_global_struct_native": """struct Pair { int x; long y; };
        const struct Pair p={5,13};
        int main(void) { return p.x*2 + p.y; }""",
    "const_local_struct_copy_native": """struct Pair { int x; long y; };
        int main(void) { struct Pair a={7,11}; const struct Pair b=a;
            a.x=1; return b.x+b.y; }""",
    "const_nested_arrays_native": """struct Grid { int data[2][2]; };
        const struct Grid g={{{1,2},{3,4}}};
        int main(void) { return g.data[1][0]*10 + g.data[0][1]; }""",
    "const_global_char_array_native": """const char text[]="abc";
        int main(void) { return text[1]-'a'; }""",
    "const_local_char_member_native": """struct Text { const char chars[5]; };
        int main(void) { struct Text text={"test"};
            return text.chars[1]-'a'; }""",
    "signed_short_sign_extend_native": """short dec(short x) { return x-1; }
        int main(void) { return dec(-123) == -124; }""",
    "unsigned_short_zero_extend_native": """unsigned short identity(unsigned short x) { return x; }
        int main(void) { return identity(65535) == 65535; }""",
    "short_assignment_narrow_native": """int main(void) { short a=-1;
        unsigned short b=(unsigned short)a; return b == 65535; }""",
    "short_arithmetic_promotion_native": """int main(void) {
        short signed_value=-101; unsigned short unsigned_value=65000;
        return signed_value+unsigned_value-64800; }""",
    "short_struct_fields_native": """struct Values { short a; unsigned short b; int c; };
        struct Values values={-11,65531,7};
        int main(void) { return values.a+values.b+values.c-65500; }""",
    "gnu_attribute_pure_nothrow_prototype": """int twice(int x)
        __attribute__((__pure__, __nothrow__));
        int twice(int x) { return x*2; }
        int main(void) { return twice(12); }""",
    "gnu_attribute_nonnull_pointer_prototype": """int read(const int *p)
        __attribute__((__pure__))
        __attribute__((__nonnull__(1)));
        int read(const int *p) { return *p; }
        int main(void) { int x=17; return read(&x); }""",
    "const_global_integer": """const int value=11;
        int main(void) { return value; }""",
    "const_global_pointer_relocation": """int value=17; int *const p=&value;
        int main(void) { return *p; }""",
    "const_member_read": """struct Pair { int x; };
        int main(void) { struct Pair a={13}; const struct Pair *p=&a;
            return p->x; }""",
    "const_conditional_pointer_merge": """int main(void) {
        int x=7; int *p=&x; const int *q=p;
        return *(1 ? p : q); }""",
    "const_local_immutable": """int main(void) {
        const int x=9; return x; }""",
    "const_pointer_reads": """int main(void) {
        int x=7; const int *p=&x; return *p; }""",
    "restrict_parameters": """int read(const int *__restrict p) { return *p; }
        int main(void) { int x=19; return read(&x); }""",
    "const_pointer_object": """int main(void) {
        int x=8; int *const p=&x; *p=5; return x; }""",
    "const_typedef_pointer": """typedef int *Ptr; int main(void) {
        int x=7; Ptr const p=&x; *p=3; return x; }""",
    "s11_longlong_signed_basic": """long long f(long long x) { return x + 1LL; }
        int main(void) { return f(4294967296LL) == 4294967297LL; }""",
    "s11_unsigned_longlong_max": """unsigned long long f(unsigned long long x) {
            return x ^ 0xffffffffffffffffULL;
        }
        int main(void) { return f(0ULL) == 18446744073709551615ULL; }""",
    "s11_longlong_rank_mixed": """int main(void) {
        long x = 3L; long long y = 4LL;
        unsigned long long z = 5ULL;
        return (x + y == 7LL) + (z > y)*2;
    }""",
    "s10_hex_unsigned_comparison": """int main(void) {
        return (0xffffffff > 0) + (0x80000000 < 0)*2
            + ((unsigned long)0xffffffff == 4294967295UL)*4;
    }""",
    "s10_hex_wide_typing": """int main(void) {
        long a = 0x100000000; unsigned long b = 0xffffffffffffffffUL;
        return (a == 4294967296L) + (b > a)*2
            + (b == (unsigned long)-1)*4;
    }""",
    "s10_octal_unsigned": """int main(void) {
        unsigned int a = 037777777777;
        return (a == 4294967295UL) + ((a >> 31) == 1)*2;
    }""",
    "s10_suffix_mixed_case": """int main(void) {
        unsigned int a = 17u; long b = 17L;
        unsigned long c = 17Lu; unsigned long d = 17Ul;
        return (a == b) + (c == d)*2 + (c == 17ul)*4;
    }""",
    "s10_unsigned_long_max": """int main(void) {
        unsigned long x = 18446744073709551615UL;
        return (x == 0xffffffffffffffff) + (x > 0)*2;
    }""",
    "hex_musl_isascii": """int isascii(int c) { return !(c&~0x7f); }
        int main(void) { return isascii(127)*4 + isascii(128)*2 + isascii(-1); }""",
    "hex_musl_iscntrl": """int iscntrl(int c) { return (unsigned)c < 0x20 || c == 0x7f; }
        int main(void) { return iscntrl(0)*8 + iscntrl(31)*4
            + iscntrl(127)*2 + iscntrl(-1); }""",
    "hex_musl_isgraph": """int isgraph(int c) { return (unsigned)c-0x21 < 0x5e; }
        int main(void) { return isgraph(33)*4 + isgraph(126)*2 + isgraph(32); }""",
    "hex_musl_isprint": """int isprint(int c) { return (unsigned)c-0x20 < 0x5f; }
        int main(void) { return isprint(32)*4 + isprint(126)*2 + isprint(127); }""",
    "hex_musl_toascii": """int toascii(int c) { return c & 0x7f; }
        int main(void) { return toascii(255)-127 + toascii(65); }""",
    "cast_musl_isdigit": """int isdigit(int c) { return (unsigned)c-'0'<10; }
        int main(void) { return isdigit('7')*10+isdigit(-1); }""",
    "cast_musl_isalpha": """int isalpha(int c) { return ((unsigned)c|32)-'a'<26; }
        int main(void) { return isalpha('A')*10+isalpha('9')+isalpha(-1); }""",
    "cast_unsigned_wrap": """int main(void) {
        unsigned int u=(unsigned)-1;
        return (u>0) + ((int)u==-1)*2 + ((unsigned char)257==1)*4;
    }""",
    "cast_typedef": """typedef unsigned long U;
        int main(void) { int x=-1; U y=(U)x; return (y>0)+((int)(unsigned char)257==1)*2; }""",
    "cast_not_lvalue": """int main(void) { int x=8; return ((unsigned)x)+1; }""",
    "ternary_lazy_true": """int main(void) {
        int x=0; int value=1 ? (x+=3) : (x+=40);
        return value*10+x; }""",
    "ternary_lazy_false": """int main(void) {
        int x=0; int value=0 ? (x+=40) : (x+=3);
        return value*10+x; }""",
    "ternary_unchosen_division": """int main(void) {
        int zero=0; return zero ? (10/zero) : 19; }""",
    "ternary_nested_associativity": """int main(void) {
        int a=0; int b=1;
        return a ? 11 : b ? 23 : 47; }""",
    "ternary_condition_side_effect": """int main(void) {
        int calls=0; int x=4; int result=(++calls) ? x+2 : x+10;
        return calls*10+result; }""",
    "ternary_integer_conversions": """int main(void) {
        unsigned char c=250; long y=7;
        return (c>200 ? c : y) / 5; }""",
    "ternary_pointer_and_null": """int main(void) {
        int x=13; int *p=0 ? 0 : &x;
        return *(1 ? p : 0); }""",
    "ternary_void_only_one_call": """int calls;
        void touch(void) { calls++; }
        int main(void) {
            int c=0; c ? touch() : touch();
            return calls; }""",
    "ternary_structure_selection": """struct Pair { int x; int y; };
        int main(void) {
            struct Pair a={3,5}; struct Pair b={7,11};
            struct Pair c=0 ? a : b; return c.x+c.y; }""",
    "ternary_true_comma_sequence": """int main(void) {
        int x=0; int result=1 ? (x+=2,x+=3) : (x+=40);
        return result*10+x; }""",
    "ternary_in_for_increment": """int main(void) {
        int i=0; int j=0;
        for (; i<4; i++, i==2 ? j++ : (j+=2)) {}
        return j; }""",
    "do_while_executes_once": """int main(void) {
        int x=3; do { x+=5; } while (0); return x; }""",
    "do_while_continue_condition": """int main(void) {
        int i=0; int checks=0;
        do { i++; if (i==1) continue; } while (checks++, i<3);
        return i*10+checks; }""",
    "do_while_break_skips_condition": """int main(void) {
        int x=0; do { x++; break; } while (x+=100);
        return x; }""",
    "do_while_nested_in_for": """int main(void) {
        int sum=0; for (int i=0; i<3; i++) {
            int j=0; do {
                j++; if (j==2) continue; sum+=i+j;
            } while (j<3);
        } return sum; }""",
    "do_while_condition_side_effect": """int main(void) {
        int x=0; int checks=0;
        do { x++; } while (checks++, x<4);
        return checks*10+x; }""",
    "do_while_empty_body": """int main(void) {
        int x=0; do ; while (x++<2); return x; }""",
    "do_while_nested_scope": """int main(void) {
        int x=9; do { int x=4; x++; } while (0); return x; }""",
    "comma_for_multiple_counters": """int main(void) {
        int i=0; int j=0; int total=0;
        for (i=0,j=0; i<5; i++, j+=3) total+=i+j;
        return total; }""",
    "comma_for_continue_step": """int main(void) {
        int i=0; int j=0; int sum=0;
        for (i=0,j=0; i<5; i++,j++) {
            if (i==2) continue;
            sum+=i+j;
        }
        return sum; }""",
    "comma_sequence_calls": """int trace;
        void mark(int n) { trace=trace*10+n; }
        int main(void) { mark(1),mark(2),mark(3); return trace; }""",
    "comma_parenthesized_call_argument": """int plus(int a,int b) { return a+b; }
        int main(void) { int x=0;
            return plus((x=4,x+1),6); }""",
    "comma_index_expression": """int main(void) {
        int values[3]; int i=0;
        values[0]=3; values[1]=11; values[2]=19;
        return values[(i=1,i)] + i; }""",
    "comma_pointer_value": """int main(void) {
        int x=7; int *p=0; return *(p=&x,p); }""",
    "comma_aggregate_initializer_separators": """int main(void) {
        int x[3]={2,3,5}; return x[0]+x[1]+x[2]; }""",
    "comma_nested_assignment": """int main(void) {
        int x=0; int y=0; return (x=3,y=4,x*y); }""",
    "for_continue_runs_increment": """int main(void) {
        int sum=0; for (int i=0; i<6; i++) {
            if (i==2) continue; sum += i;
        } return sum; }""",
    "for_nested_break_continue": """int main(void) {
        int sum=0; for (int i=0; i<4; i++)
            for (int j=0; j<5; j++) {
                if (j==1) continue; if (i==3) break;
                sum += i+j;
            }
        return sum; }""",
    "for_scope_shadowing": """int main(void) {
        int i=17; int sum=0; for (int i=0; i<4; ++i) sum += i;
        return i+sum; }""",
    "for_no_initializer": """int main(void) {
        int i=0; int sum=0; for (; i<5; i++) sum+=i;
        return sum; }""",
    "for_infinite_break": """int main(void) {
        int i=0; for (;;) { ++i; if (i==5) break; }
        return i; }""",
    "for_no_step": """int main(void) {
        int i=0; for (int j=0; j<4; ) { i+=j; j++; }
        return i; }""",
    "increment_postfix_vs_prefix": """int main(void) {
        int x=3; int old=x++; int now=++x; return old*10+now; }""",
    "decrement_postfix_vs_prefix": """int main(void) {
        int x=7; int old=x--; int now=--x; return old*10+now; }""",
    "increment_dereference_once": """int calls; int x=3;
        int *slot(void) { calls += 1; return &x; }
        int main(void) { int old=(*slot())++;
            int now=++*slot(); return calls*50+old*10+now; }""",
    "increment_pointer_array_elements": """int main(void) {
        int a[3]; a[0]=5; a[1]=17; a[2]=19;
        int *p=a; int x=*p++; int y=*++p;
        return x+y+*p; }""",
    "increment_struct_pointer_scaling": """struct Pair { int x; int y; };
        struct Pair a[3];
        int main(void) { a[0].x=3; a[1].x=7; a[2].x=11;
            struct Pair *p=a; p++; ++p; p--;
            return p->x; }""",
    "assignment_expression_narrowing": """int main(void) {
        unsigned char c=0; int saved=(c=260);
        return saved*10+c; }""",
    "assignment_expression_chain": """int main(void) {
        int a=0; int b=0; int c=0;
        int saved=(a=b=c=7); return saved+a+b+c; }""",
    "arithmetic_compound_scalar": """int main(void) {
        int a=17; int b=3; a += b *= 2; a -= 1;
        a *= 4; a /= 3; a %= 9; return a+b; }""",
    "arithmetic_compound_unsigned_char": """int main(void) {
        unsigned char c=250; c += 10; c *= 3; c /= 2;
        c %= 7; return c; }""",
    "pointer_compound_scaled_once": """int calls; int a[4]; int *p=a;
        int **slot(void) { calls += 1; return &p; }
        int main(void) { a[1]=17; a[2]=23;
            *slot() += 2; *slot() -= 1;
            return calls * 50 + *p; }""",
    "pointer_compound_struct_size": """struct Pair { int x; int y; };
        struct Pair a[3]; struct Pair *p=a;
        int main(void) { a[2].x=29; p += 2;
            p -= 1; return p[1].x; }""",
    "compound_single_eval": """int calls; int value=7;
        int *next(void) { calls=calls+1; return &value; }
        int main(void) { *next() &= 3; *next() |= 8; *next() ^= 2;
            *next() <<= 1; *next() >>= 1; return calls*10+value; }""",
    "compound_bitwise_chained": """int main(void) {
        int a=15; int b=1; a &= b |= 6; a ^= 3; return a; }""",
    "compound_shifts_narrow_unsigned": """int main(void) {
        unsigned char value=255; value &= 15; value <<= 1;
        value >>= 2; return value; }""",
    "compound_signed_shift": """int main(void) {
        int signed_value=-64; signed_value >>= 3;
        return signed_value+100; }""",
    "shift_precedence": """int main(void) {
        return ((1+2 << 3) < 30) + (32 >> 2+1) + (1 << 2 << 1);
    }""",
    "shift_signed_arithmetic": """int shr(int x, unsigned long n) { return x >> n; }
        int main(void) { return shr(-32,3) + 100; }""",
    "shift_unsigned_logical": """unsigned int shr(unsigned int x, int n) { return x >> n; }
        int main(void) { return shr(~0,28); }""",
    "shift_global_constant": """int mask = 1 << 7; int half = 128 >> 4;
        int main(void) { return mask + half; }""",
    # These must agree with GCC at execution time, not merely type-check or
    # emit valid ELF. Keep all results within the shell's 8-bit exit range.
    "bitwise_precedence": """int main(void) {
        int a=5; int b=2; int c=6;
        return (a | b ^ c & 3 == 2) + (a && b | c);
    }""",
    "bitwise_promotions": """unsigned int transform(unsigned int x, unsigned char y) {
        return (~x & 255) ^ (y | 3);
    } int main(void) { return transform(23,240) & 255; }""",
    "bitwise_constant_initializer": """int mask = (3 | 4) ^ (1 & 7);
        int main(void) { return (mask & 15) ^ (~2 & 7); }""",
    "bitwise_eager_evaluation": """int main(void) {
        int x=1; int y=2;
        return ((x | y) & 3) ^ (x && (y | 4));
    }""",
    "arithmetic": "int main(void) { int x=7; x=x*3-2; return x/2+x%2; }",
    "stack_array": """int main(void) {
        int a[4]; a[0]=3; a[1]=7; a[2]=11; a[3]=19;
        return a[1] + a[3];
    }""",
    "alias_scalar": """int main(void) {
        int x=5; int *p=&x; *p=18; return x;
    }""",
    "indexed_pointer": """int update(int *p, int idx) {
        p[idx] = 9; return *(idx+p);
    }
    int main(void) { int a[3]; a[0]=1; a[1]=2; a[2]=3;
        return update(a, 1) + a[1];
    }""",
    "multi_array": """int main(void) {
        int a[2][3]; a[0][2]=4; a[1][0]=6; a[1][2]=9;
        int *row=a[1]; return row[0]+row[2]+a[0][2];
    }""",
    "nested": """int count(int limit) {
        int i=0; int total=0;
        while (i<limit) {
            if (i==3) { i=i+1; continue; }
            if (i==7) break;
            { int total=1; if (total != 1) return 99; }
            total=total+i; i=i+1;
        }
        return total;
    } int main(void) { return count(12); }""",
    "recursive": """int factorial(int n) {
        if(n<=1) return 1;
        return n*factorial(n-1);
    } int main(void) { return factorial(5); }""",
    "mutual": """int odd(int); int even(int);
    int odd(int x) { if (x==0) return 0; return even(x-1); }
    int even(int x) { if (x==0) return 1; return odd(x-1); }
    int main(void) { return odd(9); }""",
    "long": """long wide(long n, int m) { return n / m + n % m; }
    int main(void) { return wide(100, 6); }""",
    "unsigned": """unsigned int add(unsigned int a, unsigned int b) {
        return a + b;
    } int main(void) { return add(7,11); }""",
    "short_circuit": """int main(void) {
        int n=0;
        if (n && (9/n)) return 31;
        if (1 || (9/n)) return 12;
        return 19;
    }""",
    "void_call": """void tick(void) { return; }
    int main(void) { tick(); return 17; }""",
    "prototypes": """int f(int, int);
    int main(void) { return f(5, 6); }
    int f(int x, int y) { return x + y; }""",
    "main_implicit_zero": "int main(void) { int x=2; x=x+1; }",
    "multiple_return": """int absval(int x) {
        if (x < 0) return -x;
        return x;
    } int main(void) { return absval(-7); }""",
    "signed_unsigned_comparison": """int gt(unsigned long a, unsigned long b) {
        return a > b;
    } int main(void) { return gt(5,3); }""",
    "loop_inside_branch": """int main(void) {
        int x=0; int total=0;
        if (1) { while (x<5) { total=total+x; x=x+1; } }
        else { return 99; }
        return total;
    }""",
    "external_libc": "int abs(int); int main(void) { return abs(-25); }",
    "static_function": "static int next(int x) { return x + 1; } int main(void) { return next(6); }",
    "global_initialized": "int g=7; int main(void) { return g; }",
    "global_mutation": "int g=10; int bump(void) { g=g+1; return g; } int main(void) { return bump(); }",
    "global_tentative": "int g; int main(void) { g=19; return g; }",
    "global_static": "static int g=9; extern int g; int main(void) { return g; }",
    "global_shadowing": "int g=13; int main(void) { int g=7; return g; }",
    "global_redeclaration": "extern int g; int g=42; int main(void) { return g; }",
    "global_array": "int a[4]; int main(void) { a[1]=17; return a[1]+a[0]; }",
    "global_multidim_array": "int a[2][3]; int main(void) { a[1][2]=29; return a[1][2]; }",
    "global_address_taken": "int g=3; int main(void) { int *p=&g; *p=21; return g; }",
    "global_narrow_char": "unsigned char g=255; int main(void) { return g; }",
    "global_address_initializer": "int value=37; int *ptr=&value; int main(void) { return *ptr; }",
    "global_null_pointer": "int *p=0; int main(void) { return p==0; }",
    "global_tentative_pointer": "int *p; int main(void) { return p==0; }",
    "local_string_literal": """int main(void) {
        char *p="hello"; return p[0]+p[4]-215;
    }""",
    "string_literal_escapes": r"""int main(void) {
        char *p="A\n\x42\0Z"; return p[0]+p[1]+p[2]+p[3]-141;
    }""",
    "global_string_pointer": """char *message="abc";
        int main(void) { return message[0]+message[2]-196; }""",
    "string_function_argument": """int second(char *p) { return p[1]; }
        int main(void) { return second("abc")-'b'; }""",

    "global_char_array_inferred": """char text[]="abc";
        int main(void) { return text[3]; }""",
    "global_char_array_exact": """char text[3]="abc";
        int main(void) { return text[2]-'c'; }""",
    "global_char_array_padded": """unsigned char text[16]="hi";
        int main(void) { return text[0]+text[1]+text[15]-209; }""",
    "local_char_array_inferred": """int main(void) {
        char text[]="abc"; text[0]='z'; return text[0]-'z'+text[3];
    }""",
    "local_char_array_exact": """int main(void) {
        char text[3]="abc"; return text[0]+text[2]-196;
    }""",
    "local_char_array_padded": """int main(void) {
        unsigned char text[11]="hi"; return text[10]+text[3];
    }""",
    "local_char_array_octal": r"""int main(void) {
        unsigned char data[6]="\377"; return data[0]-255+data[5];
    }""",
    "global_char_array_completed": """extern char text[];
        char text[]="abc"; extern char text[];
        int main(void) { return text[0]+text[1]+text[2]-294; }""",

    "struct_members_layout": """int main(void) {
        struct { char a; int b; long c; } v;
        v.a=5; v.b=17; v.c=31; return v.a+v.b+v.c;
    }""",
    "struct_nested": """int main(void) {
        struct { struct { char first; long last; } inner; int other; } v;
        v.inner.first=7; v.inner.last=17; v.other=11;
        return v.inner.first+v.inner.last+v.other;
    }""",
    "struct_pointer_member": """int main(void) {
        struct { int x; long y; } v; (&v)->x=13; (&v)->y=22;
        return v.x+v.y;
    }""",
    "struct_array_member": """int main(void) {
        struct { char c; long value; } a[2];
        a[1].value=23; a[0].c=7; return a[1].value+a[0].c;
    }""",
    "struct_global_zero": """struct { char first; long last; } g;
        int main(void) { g.last=29; return g.first+g.last; }""",
    "aggregate_global_int_array": """int data[4]={3,7}; int main(void) { return data[0]+data[1]+data[2]+data[3]; }""",
    "aggregate_inferred_array": """int data[]={1,2,3,4}; int main(void) { return data[2]; }""",
    "aggregate_global_struct": """struct { char a; int b; long c; } g={5,17,29}; int main(void) { return g.a+g.b+g.c; }""",
    "aggregate_local_struct": """int main(void) { struct { char a; long b; int c; } v={7,31}; return v.a+v.b+v.c; }""",
    "aggregate_pointer_fields": """int target=19;
        struct Holder { int *ptr; char *text; } value={&target,"hi"};
        int main(void) { return *value.ptr+value.text[1]-124; }""",
    "aggregate_pointer_array": """char *messages[2]={"red","blue"};
        int main(void) { return messages[0][0]+messages[1][0]; }""",
    "aggregate_self_reference": """struct Node; struct Node { int value; struct Node *next; };
        struct Node head={17,&head}; int main(void) { return head.next->value; }""",
    "enum_scalar_abi": """enum Status { A=3, B=7 };
        enum Status inc(enum Status x) { return x+1; }
        int main(void) { return inc(A); }""",
    "enum_explicit": """enum Status { FIRST, SECOND=7, THIRD=SECOND+2 };
        enum Status value=THIRD; int main(void) { return value; }""",
    "enum_negative": """int main(void) { enum Mode { NEG=-1, ZERO, THREE=ZERO+3 };
        enum Mode value=THREE; return value-NEG; }""",
    "tagged_recursive_struct": """struct Node; struct Node { int value; struct Node *next; };
        struct Node root; int main(void) { root.value=37; root.next=&root;
            return root.next->value; }""",
    "tagged_typedef": """typedef struct Item Item; struct Item { long amount; };
        Item value; int main(void) { value.amount=41; return value.amount; }""",
    "union_global": """union { char c; long l; } u={17};
        int main(void) { return u.c; }""",
    "union_local": """int main(void) {
        union { int i; long l; } u; u.i=43; return u.i;
    }""",
    "union_copy": """int main(void) {
        union { int x; long y; } u[2]; u[0].x=17; u[1]=u[0];
        return u[1].x;
    }""",
    "union_nested": """int main(void) {
        struct { int a; union { int i; long l; } u; } x;
        x.a=3; x.u.l=29; return x.a+x.u.l;
    }""",
    "aggregate_nested_array": """int main(void) { int a[2][3]={{1,2,3},{4,5,6}}; return a[0][0]+a[1][2]; }""",
    "struct_assignment": """int main(void) {
        struct { char c; int count; long total; } entries[2];
        entries[0].c=3; entries[0].count=17; entries[0].total=37;
        entries[1]=entries[0]; entries[0].count=0;
        return entries[1].c+entries[1].count+entries[1].total;
    }""",







}


def run(command: list[str], timeout: int = 30) -> dict:
    try:
        done = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        return {
            "argv": command, "returncode": done.returncode,
            "stdout": done.stdout, "stderr": done.stderr,
        }
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"argv": command, "error": str(exc)}


def test_case(name: str, src: str, args: argparse.Namespace, expected: int | None = None) -> bool:
    case = args.artifacts / name
    case.mkdir(parents=True, exist_ok=True)
    (case / "input.c").write_text(src + "\n")
    exe = case / "mini-c.exe"
    ref = case / "gcc.exe"
    obj = case / "mini-c.o"
    commands = [
        [str(args.backend), "--emit-obj", str(obj), str(case / "input.c")],
        [args.cc, str(obj), "-o", str(exe)],
        [args.cc, "-std=c11", "-O2", str(case / "input.c"), "-o", str(ref)],
        [str(exe)],
        [str(ref)],
    ]
    record = {"name": name, "commands": [], "compiler_version": run([args.cc, "--version"])}
    for index, cmd in enumerate(commands):
        result = run(cmd)
        record["commands"].append(result)
        (case / "results.json").write_text(json.dumps(record, indent=2) + "\n")
        if index < 3 and result.get("returncode") != 0:
            print(f"FAIL {name}: command #{index+1}: {result}", file=sys.stderr)
            return False
        if index >= 3 and "returncode" not in result:
            print(f"FAIL {name}: execution failed: {result}", file=sys.stderr)
            return False
    mini, gcc = record["commands"][3:5]
    # A matching signal crash is not a successful differential comparison.
    if (
        mini["returncode"] < 0 or gcc["returncode"] < 0
        or (expected is not None and gcc["returncode"] != expected)
        or mini["returncode"] != gcc["returncode"]
        or mini["stdout"] != gcc["stdout"]
    ):
        print(f"FAIL {name}: Mini-C {mini}, GCC {gcc}, expected={expected}", file=sys.stderr)
        return False
    shutil.rmtree(case)
    return True



def mixed_objects(name: str, mini_source: str, gcc_source: str, args: argparse.Namespace) -> bool:
    """Validate SysV function ABI in both linking directions."""
    directory = args.artifacts / name
    directory.mkdir(parents=True, exist_ok=True)
    mini_input = directory / "mini.c"
    gcc_input = directory / "gcc.c"
    mini_input.write_text(mini_source + "\n")
    gcc_input.write_text(gcc_source + "\n")
    obj = directory / "mini.o"
    executable = directory / "combined.exe"
    reference = directory / "reference.exe"
    commands = [
        [str(args.backend), "--emit-obj", str(obj), str(mini_input)],
        [args.cc, str(obj), str(gcc_input), "-o", str(executable)],
        [args.cc, "-std=c11", "-O2", str(mini_input), str(gcc_input), "-o", str(reference)],
        [str(executable)],
        [str(reference)],
    ]
    records = []
    for idx, command in enumerate(commands):
        result = run(command)
        records.append(result)
        (directory / "results.json").write_text(
            json.dumps({"case": name, "commands": records}, indent=2) + "\n"
        )
        if idx < 3 and result.get("returncode") != 0:
            print(f"FAIL {name}: link/compile: {result}", file=sys.stderr)
            return False
        if idx >= 3 and "returncode" not in result:
            print(f"FAIL {name}: execution: {result}", file=sys.stderr)
            return False
    if (
        records[-2]["returncode"] < 0 or records[-1]["returncode"] < 0
        or records[-2]["returncode"] != records[-1]["returncode"]
        or records[-2]["stdout"] != records[-1]["stdout"]
    ):
        print(f"FAIL {name}: ABI mismatch: {records[-2]} vs {records[-1]}", file=sys.stderr)
        return False
    shutil.rmtree(directory)
    return True

def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--backend", type=Path, default=Path("target/debug/mini-c-cranelift"))
    p.add_argument("--fuzzer", type=Path, default=Path("target/debug/mini-c-fuzz"))
    p.add_argument("--seed-count", type=int, default=4)
    p.add_argument("--steps", type=int, default=100)
    p.add_argument("--cc", default="cc")
    p.add_argument("--artifacts", type=Path, default=Path("target/cranelift-failures"))
    args = p.parse_args()
    if args.seed_count < 0 or args.steps < 0:
        p.error("seed-count and steps must be nonnegative")
    args.backend = args.backend.resolve()
    args.fuzzer = args.fuzzer.resolve()
    args.artifacts = args.artifacts.resolve()
    if shutil.which(args.cc) is None or not args.backend.is_file():
        print("build the backend and ensure cc is installed", file=sys.stderr)
        return 2
    if args.seed_count and not args.fuzzer.is_file():
        print("build mini-c-fuzz before running seeded differential tests", file=sys.stderr)
        return 2
    for name, program in PROGRAMS.items():
        if not test_case(name, program, args):
            print(f"Failure reproducer retained: {args.artifacts / name}", file=sys.stderr)
            return 1
    mixed = [
        # Verify both directions of the native SysV ABI, not only whole-TU
        # Mini-C compilation (which can hide symmetric ABI mistakes).
        # GCC-defined variadic functions explicitly inspect va_arg(double).
        # The fixed SSE argument must also be counted in AL.
        ("mini_calls_gcc_variadic_fixed_sse_and_ellipsis",
         "extern int sum_variadic(double fixed, ...); int main(void) { return sum_variadic(1.5,2.5)!=4; }",
         "#include <stdarg.h>\nint sum_variadic(double fixed, ...) { va_list ap; va_start(ap,fixed); double x=va_arg(ap,double); va_end(ap); return (int)(fixed+x); }"),
        ("mini_calls_gcc_variadic_mixed_stack_gp_sse",
         "extern int accumulate(int fixed, ...); int main(void) { return accumulate(1,2.5,3,4.5)!=11; }",
         "#include <stdarg.h>\nint accumulate(int fixed, ...) { va_list ap; va_start(ap,fixed); double a=va_arg(ap,double); int b=va_arg(ap,int); double c=va_arg(ap,double); va_end(ap); return fixed+(int)a+b+(int)c; }"),
        # Cross-object SSE float/double scalar calls and returns.
        ("gcc_calls_mini_f64",
         "double twice(double x) { return x*2.0; }",
         "double twice(double); int main(void) { return twice(3.5)!=7.0; }"),
        ("mini_calls_gcc_f32",
         "extern float sum(float,float); int main(void) { return sum(2.5f,4.0f)!=6.5f; }",
         "float sum(float a,float b) { return a+b; }"),
        ("gcc_calls_mini_gp_sse_mix",
         "double combine(int a,double b,float c) { return (double)a+b+(double)c; }",
         "double combine(int,double,float); int main(void) { return combine(3,2.5,4.5f)!=10.0; }"),
        # Independently compiled GCC/Mini-C MEMORY-aggregate ABI in both directions.
        ("gcc_calls_mini_twenty_byte_stack_struct",
         "struct Odd { int a,b,c,d,e; }; int sum(struct Odd x) { return x.a+x.b+x.c+x.d+x.e; }",
         "struct Odd { int a,b,c,d,e; }; int sum(struct Odd); int main(void) { struct Odd x={1,2,3,4,5}; return sum(x)!=15; }"),
        ("mini_calls_gcc_twenty_byte_stack_struct",
         "struct Odd { int a,b,c,d,e; }; extern int sum(struct Odd); int main(void) { struct Odd x={1,2,3,4,5}; return sum(x)!=15; }",
         "struct Odd { int a,b,c,d,e; }; int sum(struct Odd x) { return x.a+x.b+x.c+x.d+x.e; }"),
        ("gcc_calls_mini_sret_large",
         "struct Large { long a,b,c; }; struct Large make(long x,long y,long z) { struct Large a={x,y,z}; return a; }",
         "struct Large { long a,b,c; }; struct Large make(long,long,long); int main(void) { struct Large x=make(7,11,13); return x.a!=7 || x.b!=11 || x.c!=13; }"),
        ("mini_calls_gcc_sret_large",
         "struct Large { long a,b,c; }; extern struct Large make(long,long,long); int main(void) { struct Large x=make(7,11,13); return x.a!=7 || x.b!=11 || x.c!=13; }",
         "struct Large { long a,b,c; }; struct Large make(long x,long y,long z) { struct Large a={x,y,z}; return a; }"),
        ("gcc_calls_mini_stack_struct",
         "struct Large { long a,b,c; }; long sum(struct Large x) { return x.a*2+x.b*3+x.c; }",
         "struct Large { long a,b,c; }; long sum(struct Large); int main(void) { struct Large x={7,11,13}; return sum(x)!=60; }"),
        ("mini_calls_gcc_stack_struct",
         "struct Large { long a,b,c; }; extern long sum(struct Large); int main(void) { struct Large x={7,11,13}; return sum(x)!=60; }",
         "struct Large { long a,b,c; }; long sum(struct Large x) { return x.a*2+x.b*3+x.c; }"),
        ("gcc_calls_mini_memory_roundtrip",
         "struct Large { long a,b,c; }; struct Large rotate(struct Large x) { struct Large y={x.b,x.c,x.a}; return y; }",
         "struct Large { long a,b,c; }; struct Large rotate(struct Large); int main(void) { struct Large x={7,11,13}; x=rotate(x); return x.a!=11 || x.b!=13 || x.c!=7; }"),
        ("mini_calls_gcc_register_exhaustion_struct",
         "struct Pair { long a,b; }; extern long f(long,long,long,long,long,long,struct Pair); int main(void) { struct Pair p={7,11}; return f(1,2,3,4,5,6,p)!=39; }",
         "struct Pair { long a,b; }; long f(long a,long b,long c,long d,long e,long g,struct Pair p) { return a+b+c+d+e+g+p.a+p.b; }"),
        # Cross-object SysV SSE and mixed integer/SSE aggregate cases.
        ("gcc_calls_mini_mixed_sse_integer_struct",
         "struct Mix { double d; long n; }; struct Mix apply(struct Mix x) { x.d+=1.5; x.n+=4; return x; }",
         "struct Mix { double d; long n; }; struct Mix apply(struct Mix); int main(void) { struct Mix x; x.d=2.5; x.n=7; x=apply(x); return x.d!=4.0 || x.n!=11; }"),
        ("mini_calls_gcc_mixed_integer_sse_struct",
         "struct Mix { long n; double d; }; extern struct Mix apply(struct Mix); int main(void) { struct Mix x; x.n=7; x.d=2.5; x=apply(x); return x.n!=11 || x.d!=4.0; }",
         "struct Mix { long n; double d; }; struct Mix apply(struct Mix x) { x.d+=1.5; x.n+=4; return x; }"),
        ("gcc_calls_mini_two_float_struct",
         "struct PairF { float a,b; }; struct PairF flip(struct PairF x) { float t=x.a; x.a=x.b; x.b=t; return x; }",
         "struct PairF { float a,b; }; struct PairF flip(struct PairF); int main(void) { struct PairF x; x.a=1.5f; x.b=3.25f; x=flip(x); return x.a!=3.25f || x.b!=1.5f; }"),
        ("mini_calls_gcc_double_pair_struct",
         "struct PairD { double a,b; }; extern struct PairD adjust(struct PairD); int main(void) { struct PairD x; x.a=1.25; x.b=3.5; x=adjust(x); return x.a!=2.25 || x.b!=5.5; }",
         "struct PairD { double a,b; }; struct PairD adjust(struct PairD x) { x.a+=1.0; x.b+=2.0; return x; }"),
        # Genuine GCC interop of C by-value aggregate arguments and returns.
        ("gcc_calls_mini_aggregate_two_registers",
         "struct Pair { long a,b; }; struct Pair adjust(struct Pair p) { p.a+=5; p.b-=3; return p; }",
         "struct Pair { long a,b; }; struct Pair adjust(struct Pair); int main(void) { struct Pair p={7,23}; struct Pair q=adjust(p); return q.a!=12 || q.b!=20; }"),
        ("mini_calls_gcc_aggregate_two_registers",
         "struct Pair { long a,b; }; extern struct Pair adjust(struct Pair); int main(void) { struct Pair p={7,23}; struct Pair q=adjust(p); return q.a!=12 || q.b!=20; }",
         "struct Pair { long a,b; }; struct Pair adjust(struct Pair p) { p.a+=5; p.b-=3; return p; }"),
        ("gcc_calls_mini_aggregate_narrow",
         "struct Tiny { char a; short b; }; struct Tiny adjust(struct Tiny p) { p.a+=1; p.b+=3; return p; }",
         "struct Tiny { char a; short b; }; struct Tiny adjust(struct Tiny); int main(void) { struct Tiny p={7,23}; struct Tiny q=adjust(p); return q.a!=8 || q.b!=26; }"),
        ("mini_calls_gcc_aggregate_twelve_bytes",
         "struct Three { int a,b,c; }; extern struct Three add(struct Three); int main(void) { struct Three p={2,5,11}; struct Three q=add(p); return q.a!=3 || q.b!=5 || q.c!=14; }",
         "struct Three { int a,b,c; }; struct Three add(struct Three x) { x.a++; x.c+=3; return x; }"),
        ("gcc_calls_mini_short_params",
         "int combine(short a, unsigned short b) { return a+b; }",
         "extern int combine(short,unsigned short); int main(void) { return combine(-5,65000)==64995; }"),
        ("mini_calls_gcc_short_params",
         "extern int combine(short,unsigned short); int main(void) { return combine(-12,65000)==64988; }",
         "int combine(short a,unsigned short b) { return a+b; }"),
        ("gcc_reads_mini_const_struct_init",
         "struct Pair { int x; long y; }; const struct Pair shared={7,13};",
         "extern const struct Pair { int x; long y; } shared; int main(void) { return shared.x*2+shared.y; }"),
        ("gcc_reads_mini_const_string",
         'const char text[]="abc";',
         "extern const char text[]; int main(void) { return text[1]; }"),
        ("gcc_reads_mini_unsigned_short",
         "unsigned short shared=65530;",
         "extern unsigned short shared; int main(void) { return shared-65400; }"),
        ("mini_reads_gcc_signed_short",
         "extern short shared; int main(void) { return shared+200; }",
         "short shared=-101;"),
        ("gcc_calls_mini", "int kernel(int x, int y) { return x * y + y; }",
         "int kernel(int,int); int main(void) { return kernel(6,7); }"),
        ("mini_calls_gcc", "int supply(int); int main(void) { return supply(20); }",
         "int supply(int x) { return x + 3; }"),
        ("mini_reads_gcc_global",
         "extern int shared; int main(void) { return shared; }",
         "int shared=27;"),
        ("mini_writes_gcc_global",
         "extern int shared; int main(void) { shared=31; return shared; }",
         "int shared=5;"),
        ("gcc_reads_mini_global",
         "int shared=23;",
         "extern int shared; int main(void) { return shared; }"),
        ("gcc_writes_mini_global",
         "int shared;",
         "extern int shared; int main(void) { shared=37; return shared; }"),
        ("mini_reads_gcc_pointer_global",
         "extern int *shared; int main(void) { return *shared; }",
         "int value=43; int *shared=&value;"),
        ("gcc_reads_mini_null_pointer_global",
         "int *shared=0;",
         "extern int *shared; int main(void) { return shared==0 ? 0 : 1; }"),
        ("gcc_writes_mini_pointer_global",
         "int *shared;",
         "extern int *shared; int main(void) { int value=19; shared=&value; return *shared; }"),
        ("gcc_reads_mini_relocated_pointer",
         "int value=41; int *shared=&value;",
         "extern int *shared; int main(void) { return *shared; }"),
        ("mini_reads_gcc_relocated_pointer",
         "extern int value; int *shared=&value; int main(void) { return *shared; }",
         "int value=29;"),
        ("gcc_reads_mini_array_decay_pointer",
         "int values[3]; int *shared=values; int set(void) { values[1]=53; return 0; }",
         "extern int *shared; int set(void); int main(void) { set(); return shared[1]; }"),
        ("mini_reads_gcc_array_decay_pointer",
         "extern int values[3]; int *shared=values; int main(void) { return shared[1]; }",
         "int values[3]={0};"),
        ("mini_static_relocated_pointer",
         "static int value=17; static int *shared=&value; int get(void) { return *shared; }",
         "int get(void); int main(void) { return get(); }"),
        ("gcc_reads_mini_string_pointer",
         'char *shared="AZ";',
         "extern char *shared; int main(void) { return shared[1]-'Z'; }"),
        ("mini_passes_string_to_gcc",
         'extern int second(char *); int main(void) { return second("abc")-98; }',
         "int second(char *p) { return p[1]; }"),
        ("gcc_reads_mini_character_array",
         'char text[]="abc";',
         "extern char text[]; int main(void) { return text[2]-'c'; }"),
        ("mini_reads_gcc_character_array",
         "extern char text[4]; int main(void) { return text[3]; }",
         'char text[4]="abc";'),

        ("gcc_reads_mini_struct",
         "struct Record { char a; int b; long c; } shared; int put(void) { shared.b=17; shared.c=23; return 0; }",
         "extern struct Record { char a; int b; long c; } shared; int put(void); int main(void) { put(); return shared.b+shared.c; }"),
        ("gcc_reads_mini_aggregate_pointer_relocations",
         'int target=29; struct Holder { int *ptr; char *text; } value={&target,"abc"};',
         'extern struct Holder { int *ptr; char *text; } value; int main(void) { return *value.ptr+value.text[2]; }'),
        ("gcc_calls_mini_enum_scalar",
         'enum Status { A=3 }; enum Status inc(enum Status x) { return x+1; }',
         'enum Status { A=3 }; enum Status inc(enum Status); int main(void) { return inc(A); }'),
        ("mini_reads_gcc_struct_layout",
         "extern struct Record { char a; int b; long c; } shared; int main(void) { return shared.b+shared.c; }",
         "struct Record { char a; int b; long c; } shared={0,17,29};"),
        ("gcc_reads_mini_union_layout",
         "union Data { long wide; int narrow; } shared; int init(void) { shared.wide=31; return 0; }",
         "extern union Data { long wide; int narrow; } shared; int init(void); int main(void) { init(); return shared.wide; }"),
        ("mini_reads_gcc_union_layout",
         "extern union Data { long wide; int narrow; } shared; int main(void) { return shared.wide; }",
         "union Data { long wide; int narrow; } shared={41};"),
        ("gcc_follows_mini_recursive_struct",
         "struct Node { int value; struct Node *next; }; struct Node root; int init(void) { root.value=37; root.next=&root; return 0; }",
         "extern struct Node { int value; struct Node *next; } root; int init(void); int main(void) { init(); return root.next->value; }"),
        ("mini_static_global_not_exported",
         "static int hidden=11; int get_hidden(void) { return hidden; }",
         "int get_hidden(void); int main(void) { return get_hidden(); }"),
    ]
    for name, mini_source, gcc_source in mixed:
        if not mixed_objects(name, mini_source, gcc_source, args):
            print(f"Failure reproducer retained: {args.artifacts / name}", file=sys.stderr)
            return 1
    for seed in range(args.seed_count):
        generated = run([str(args.fuzzer), str(seed), str(args.steps)])
        if generated.get("returncode") != 0:
            print(f"FAIL generating seed={seed}: {generated}", file=sys.stderr)
            return 1
        if not test_case(f"seed-{seed}", generated["stdout"], args, expected=1):
            print(f"Failure reproducer retained: {args.artifacts / f'seed-{seed}'}", file=sys.stderr)
            return 1
    print(f"PASS {len(PROGRAMS)} standalone fixtures, {len(mixed)} mixed-object ABI cases, "
          f"and {args.seed_count} generated programs")
    return 0


if __name__ == "__main__":
    sys.exit(main())
