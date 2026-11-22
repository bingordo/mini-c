#!/usr/bin/env python3
"""GCC/Mini-C C99 bit-field aggregate initializer native execution differentials."""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = {
    "continue_global_elided_nested_struct": r"""
        struct N {int first;int second;};
        struct S {int a[2];struct N pair;int tail;};
        struct S g={.a[1]=3,4,5,6};
        int main(void) {
            return g.a[0]!=0 || g.a[1]!=3 || g.pair.first!=4
                || g.pair.second!=5 || g.tail!=6;
        }
    """,
    "continue_local_elided_union_array": r"""
        struct N {int a;int b;};
        struct W {union U {struct N n;long raw;} items[2];int tail;};
        int main(void) {
            struct W w={.items[0].n.b=4,5,6,7};
            return w.items[0].n.a!=0 || w.items[0].n.b!=4
                || w.items[1].n.a!=5 || w.items[1].n.b!=6
                || w.tail!=7;
        }
    """,
    "continue_global_array_middle": r"""
        struct S {int a[3];int tail;};
        struct S g={.a[0]=3,4,5,6};
        int main(void) {
            return g.a[0]!=3 || g.a[1]!=4 || g.a[2]!=5 || g.tail!=6;
        }
    """,
    "continue_global_array_end_climbs": r"""
        struct S {int a[3];int tail;};
        struct S g={.a[2]=3,4};
        int main(void) {
            return g.a[0]!=0 || g.a[1]!=0 || g.a[2]!=3 || g.tail!=4;
        }
    """,
    "continue_global_nested_struct": r"""
        struct N {int x;int y;};
        struct W {struct N n;int tail;};
        struct W g={.n.x=3,4,5};
        int main(void) {return g.n.x!=3 || g.n.y!=4 || g.tail!=5;}
    """,
    "continue_global_nested_array_matrix": r"""
        int g[2][3]={[0][2]=3,4,5,6};
        int main(void) {
            return g[0][0]!=0 || g[0][1]!=0 || g[0][2]!=3
                || g[1][0]!=4 || g[1][1]!=5 || g[1][2]!=6;
        }
    """,
    "continue_global_bitfield_same_unit": r"""
        struct B {unsigned a:3,b:5,c:7;};
        struct W {struct B bits;int tail;};
        struct W g={.bits.a=3,5,6,7};
        int main(void) {
            return g.bits.a!=3 || g.bits.b!=5 || g.bits.c!=6
                || g.tail!=7;
        }
    """,
    "continue_global_bitfield_mixed_units": r"""
        struct B {_Bool a:1;unsigned short b:11;unsigned long c:37;};
        struct W {struct B bits;int tail;};
        struct W g={.bits.a=1,511,1234567UL,99};
        int main(void) {
            return g.bits.a!=1 || g.bits.b!=511
                || g.bits.c!=1234567UL || g.tail!=99;
        }
    """,
    "continue_global_bitfields_array": r"""
        struct B {unsigned a:3,b:5;};
        struct W {struct B bits[2];int tail;};
        struct W g={.bits[1].a=3,17,9};
        int main(void) {
            return g.bits[0].a!=0 || g.bits[0].b!=0
                || g.bits[1].a!=3 || g.bits[1].b!=17
                || g.tail!=9;
        }
    """,
    "continue_local_union_array": r"""
        union U {int a[2];long raw;};
        int main(void) {
            union U u={.a[0]=3,4};
            return u.a[0]!=3 || u.a[1]!=4;
        }
    """,
    "continue_local_union_to_outer_sibling": r"""
        struct N {int a;int b;};
        struct W {union U {struct N n;long raw;} u;int tail;};
        int main(void) {
            struct W w={.u.n.a=3,4,5};
            return w.u.n.a!=3 || w.u.n.b!=4 || w.tail!=5;
        }
    """,
    "continue_local_restart_designator": r"""
        int main(void) {
            int matrix[2][2]={[1][1]=9,[0][0]=3,4,5};
            return matrix[0][0]!=3 || matrix[0][1]!=4
                || matrix[1][0]!=5 || matrix[1][1]!=9;
        }
    """,
    "continue_local_overlapping_paths": r"""
        struct S {int a[3];int tail;};
        int main(void) {
            struct S v={.a[0]=3,4,.a[0]=7,5};
            return v.a[0]!=7 || v.a[1]!=5
                || v.a[2]!=0 || v.tail!=0;
        }
    """,
    "continue_local_side_effect_count": r"""
        int calls;
        int effect(void){calls++;return 3;}
        struct B {unsigned a:3,b:5;};
        struct W {struct B bits;int tail;};
        int main(void) {
            struct W x={.bits.a=effect(),effect(),9};
            return calls!=2 || x.bits.a!=3 || x.bits.b!=3 || x.tail!=9;
        }
    """,
    "continue_compound_literal_bitfields": r"""
        struct B {unsigned a:3,b:5;};
        struct W {struct B bits;int tail;};
        int main(void) {
            struct W *p=&(struct W){.bits.a=3,17,9};
            return p->bits.a!=3 || p->bits.b!=17 || p->tail!=9;
        }
    """,
    "continue_global_braced_parent_boundary": r"""
        struct N {int a;int b;};
        struct W {struct N n;int tail;};
        struct W g={.n={3,4},5};
        int main(void){return g.n.a!=3 || g.n.b!=4 || g.tail!=5;}
    """,
    "continue_global_union_inside_array": r"""
        struct N {int a;int b;};
        struct W {union U {struct N n;long raw;} items[2];int tail;};
        struct W g={.items[0].n.a=3,4, .items[1].n.a=5,6,7};
        int main(void) {
            return g.items[0].n.a!=3 || g.items[0].n.b!=4
                || g.items[1].n.a!=5 || g.items[1].n.b!=6 || g.tail!=7;
        }
    """,
    "flat_elided_global_matrix": r"""
        int a[2][3]={1,2,3,4,5,6};
        int main(void) {
            return a[0][0]!=1 || a[0][1]!=2 || a[0][2]!=3
                || a[1][0]!=4 || a[1][1]!=5 || a[1][2]!=6;
        }
    """,
    "flat_elided_local_matrix": r"""
        int main(void) {
            int a[2][2]={1,2,3,4};
            return a[0][0]!=1 || a[0][1]!=2 || a[1][0]!=3 || a[1][1]!=4;
        }
    """,
    "flat_elided_global_nested_struct": r"""
        struct N {int a;int b;};
        struct W {struct N n;int tail;};
        struct W w={3,4,5};
        int main(void) {return w.n.a!=3 || w.n.b!=4 || w.tail!=5;}
    """,
    "flat_elided_local_nested_struct": r"""
        struct N {int a;int b;};
        struct W {struct N n;int tail;};
        int main(void) {
            struct W w={3,4,5};
            return w.n.a!=3 || w.n.b!=4 || w.tail!=5;
        }
    """,
    "flat_elided_global_bitfields": r"""
        struct B {unsigned a:3,b:5,c:7;};
        struct W {struct B bits;int tail;};
        struct W w={3,17,59,9};
        int main(void) {
            return w.bits.a!=3 || w.bits.b!=17 || w.bits.c!=59 || w.tail!=9;
        }
    """,
    "flat_elided_local_bitfields": r"""
        struct B {unsigned a:3,b:5;};
        struct W {struct B bits;int tail;};
        int main(void) {
            struct W w={3,17,9};
            return w.bits.a!=3 || w.bits.b!=17 || w.tail!=9;
        }
    """,
    "flat_elided_global_mixed_units": r"""
        struct B {_Bool a:1;unsigned short b:11;unsigned long c:37;};
        struct W {struct B bits;int tail;};
        struct W w={1,511,1234567UL,99};
        int main(void) {
            return w.bits.a!=1 || w.bits.b!=511
                || w.bits.c!=1234567UL || w.tail!=99;
        }
    """,
    "flat_elided_global_union_struct": r"""
        struct N {int a;int b;};
        union U {struct N n;long raw;};
        union U u={3,4};
        int main(void) {return u.n.a!=3 || u.n.b!=4;}
    """,
    "flat_elided_local_union_in_struct": r"""
        struct N {int a;int b;};
        struct W {union U {struct N n;long raw;} u;int tail;};
        int main(void) {
            struct W w={3,4,5};
            return w.u.n.a!=3 || w.u.n.b!=4 || w.tail!=5;
        }
    """,
    "flat_elided_designated_array": r"""
        int a[2][2]={[0]=1,2,3,4};
        int main(void) {
            return a[0][0]!=1 || a[0][1]!=2 || a[1][0]!=3 || a[1][1]!=4;
        }
    """,
    "flat_elided_designated_struct": r"""
        struct N {int a;int b;};
        struct W {struct N n;int tail;};
        struct W w={.n=3,4,5};
        int main(void) {return w.n.a!=3 || w.n.b!=4 || w.tail!=5;}
    """,
    "flat_elided_mixed_explicit_braces": r"""
        int a[2][2]={1,2,{3,4}};
        int main(void) {
            return a[0][0]!=1 || a[0][1]!=2 || a[1][0]!=3 || a[1][1]!=4;
        }
    """,
    "flat_elided_array_of_struct": r"""
        struct N {int a;int b;};
        struct N a[2]={3,4,5,6};
        int main(void) {
            return a[0].a!=3 || a[0].b!=4 || a[1].a!=5 || a[1].b!=6;
        }
    """,
    "flat_elided_local_compound_literal": r"""
        struct B {unsigned a:3,b:5;};
        struct W {struct B bits;int tail;};
        int main(void) {
            struct W *p=&(struct W){3,17,9};
            return p->bits.a!=3 || p->bits.b!=17 || p->tail!=9;
        }
    """,
    "flat_elided_local_side_effects": r"""
        int calls;
        int effect(void) {calls++;return calls;}
        struct N {int a;int b;};
        struct W {struct N n;int tail;};
        int main(void) {
            struct W w={effect(),effect(),effect()};
            return calls!=3 || w.n.a!=1 || w.n.b!=2 || w.tail!=3;
        }
    """,
    "flat_whole_struct_copy_preserved": r"""
        struct N {int a;int b;};
        struct W {struct N n;int tail;};
        int main(void) {
            struct N source={3,4};
            struct W w={source,9};
            return w.n.a!=3 || w.n.b!=4 || w.tail!=9;
        }
    """,
    "flat_string_array_not_scalar_elided": r"""
        struct S {char bytes[5];int tail;};
        struct S s={"abc",7};
        int main(void) {
            return s.bytes[0]!='a' || s.bytes[1]!='b'
                || s.bytes[2]!='c' || s.bytes[3]!=0 || s.bytes[4]!=0
                || s.tail!=7;
        }
    """,
    "override_global_bitfield_exact": r"""
        struct B {unsigned a:3,b:5;};
        struct B x={.a=1,.b=17,.a=6};
        int main(void) {return x.a!=6 || x.b!=17;}
    """,
    "override_global_bitfield_same_unit": r"""
        struct B {unsigned short a:5,b:9;};
        struct B x={.a=3,.b=257,.a=13,.b=383};
        int main(void) {return x.a!=13 || x.b!=383 || sizeof(x)!=2;}
    """,
    "override_global_mixed_bitfield_types": r"""
        struct B {_Bool a:1;unsigned int b:3;unsigned long c:37;};
        struct B x={.a=1,.c=1234567UL,.b=3,.a=0,.c=987654UL};
        int main(void) {return x.a!=0 || x.b!=3 || x.c!=987654UL;}
    """,
    "override_global_union_switch_arm": r"""
        union U {unsigned int a:3,b:5;long raw;};
        union U u={.a=5,.raw=0,.b=23};
        int main(void) {return u.b!=23 || sizeof(u)!=8;}
    """,
    "override_global_nested_bitfield_exact": r"""
        struct B {unsigned a:3,b:5;};
        struct Box {struct B bits[2];int tail;};
        struct Box box={.bits[0].a=1,.bits[1].b=17,
                        .bits[0].a=6,.bits[0].b=13,.tail=7};
        int main(void) {
            return box.bits[0].a!=6 || box.bits[0].b!=13
                || box.bits[1].a!=0 || box.bits[1].b!=17
                || box.tail!=7;
        }
    """,
    "override_global_nested_union_arm": r"""
        union U {struct B {unsigned a:3,b:5;} bits; unsigned long raw;};
        union U u={.bits.a=5,.bits.b=23,.raw=41};
        int main(void) {return u.raw!=41UL;}
    """,
    "override_local_bitfield_implicit_then_designated": r"""
        struct B {unsigned a:3,b:5;};
        int main(void) {
            struct B x={1,17,.a=6};
            return x.a!=6 || x.b!=17;
        }
    """,
    "override_local_array_duplicate": r"""
        struct B {unsigned a:3,b:5;};
        int main(void) {
            struct B x[2]={[1].a=1,[0].b=11,[1].a=5,[1].b=19};
            return x[0].a!=0 || x[0].b!=11
                || x[1].a!=5 || x[1].b!=19;
        }
    """,
    "override_local_nested_union_arm": r"""
        struct Wrap {union U {unsigned a:3,b:5;} u;int tail;};
        int main(void) {
            struct Wrap x={.u.a=2,.u.b=17,.tail=12};
            return x.u.b!=17 || x.tail!=12;
        }
    """,
    "override_local_side_effect_discarded": r"""
        int called;
        int effect(void) {called++;return 1;}
        struct B {unsigned a:3,b:5;};
        int main(void) {
            struct B x={.a=effect(),.a=5,.b=17};
            return called!=0 || x.a!=5 || x.b!=17;
        }
    """,
    "override_local_union_effect_discarded": r"""
        int called;
        int effect(void) {called++;return 3;}
        union U {unsigned int a:3,b:5;};
        int main(void) {
            union U x={.a=effect(),.b=19};
            return called!=0 || x.b!=19;
        }
    """,
    "override_compound_literal_bitfield": r"""
        struct B {unsigned a:3,b:5;};
        int main(void) {
            struct B *x=&(struct B){.a=1,.b=17,.a=4};
            return x->a!=4 || x->b!=17;
        }
    """,
    "parent_child_global_bitfields": r"""
        struct B {unsigned a:3,b:5;};
        struct Box {struct B bits;int tail;};
        struct Box g={.bits={5,17},.bits.b=23,.tail=9};
        int main(void) {
            return g.bits.a!=5 || g.bits.b!=23 || g.tail!=9;
        }
    """,
    "parent_child_global_reverse_zero": r"""
        struct B {unsigned a:3,b:5;};
        struct Box {struct B bits;int tail;};
        struct Box g={.bits.b=23,.bits={.a=4}};
        int main(void) {
            return g.bits.a!=4 || g.bits.b!=0 || g.tail!=0;
        }
    """,
    "parent_child_global_parent_reset": r"""
        struct B {unsigned a:3,b:5;};
        struct Box {struct B bits;int tail;};
        struct Box g={.bits={3,17},.bits={.a=6}};
        int main(void) {
            return g.bits.a!=6 || g.bits.b!=0 || g.tail!=0;
        }
    """,
    "parent_child_global_mixed_unit_array": r"""
        struct B {_Bool a:1;unsigned short b:11;unsigned long c:37;};
        struct Box {struct B bits[2];int tail;};
        struct Box g={.bits[0]={1,511,1234567UL},
                      .bits[0].b=1001,.bits[1].c=9999999UL,.tail=7};
        int main(void) {
            return g.bits[0].a!=1 || g.bits[0].b!=1001
                || g.bits[0].c!=1234567UL || g.bits[1].a!=0
                || g.bits[1].b!=0 || g.bits[1].c!=9999999UL
                || g.tail!=7;
        }
    """,
    "parent_child_global_nested_union": r"""
        struct B {unsigned a:3,b:5;};
        struct Box {union U {struct B bits;long raw;} u;int tail;};
        struct Box g={.u.bits={3,17},.u.bits.b=19,.tail=9};
        int main(void) {
            return g.u.bits.a!=3 || g.u.bits.b!=19 || g.tail!=9;
        }
    """,
    "parent_child_global_nested_union_replacement": r"""
        struct B {unsigned a:3,b:5;};
        struct Box {union U {struct B bits;long raw;} u;int tail;};
        struct Box g={.u.bits={3,17},.u.raw=99,.tail=9};
        int main(void) {
            return g.u.raw!=99 || g.tail!=9;
        }
    """,
    "parent_child_local_bitfield_side_effects": r"""
        int ca;int cb;
        int effect_a(void){ca++;return 3;}
        int effect_b(void){cb++;return 17;}
        struct B {unsigned a:3,b:5;};
        struct Box {struct B bits;int tail;};
        int main(void) {
            struct Box x={.bits={effect_a(),effect_b()},
                          .bits.b=23,.tail=9};
            return ca!=1 || cb!=0 || x.bits.a!=3
                || x.bits.b!=23 || x.tail!=9;
        }
    """,
    "parent_child_local_reverse_omits_effect": r"""
        int calls;
        int effect(void){calls++;return 3;}
        struct B {unsigned a:3,b:5;};
        struct Box {struct B bits;};
        int main(void) {
            struct Box x={.bits.b=effect(),.bits={.a=4}};
            return calls!=0 || x.bits.a!=4 || x.bits.b!=0;
        }
    """,
    "parent_child_local_array_override": r"""
        int main(void) {
            int a[2][2]={[1]={3,4},[1][0]=5};
            return a[0][0]!=0 || a[0][1]!=0 || a[1][0]!=5 || a[1][1]!=4;
        }
    """,
    "parent_child_local_copy_after_child": r"""
        struct B {unsigned a:3,b:5;};
        struct Box {struct B bits;};
        int main(void) {
            struct B source={4,13};
            struct Box x={.bits.b=5,.bits=source};
            return x.bits.a!=4 || x.bits.b!=13;
        }
    """,
    "parent_child_global_string_array_override": r"""
        struct Word {char text[5];unsigned a:3;};
        struct Word g={.text="ABC",.text[1]='Z',.a=5};
        int main(void) {
            return g.text[0]!='A' || g.text[1]!='Z'
                || g.text[2]!='C' || g.text[3]!=0
                || g.text[4]!=0 || g.a!=5;
        }
    """,
    "parent_child_local_compound_literal": r"""
        struct B {unsigned a:3,b:5;};
        struct Box {struct B bits;int tail;};
        int main(void) {
            struct Box *x=&(struct Box){
                .bits={5,17},.bits.b=23,.tail=9
            };
            return x->bits.a!=5 || x->bits.b!=23 || x->tail!=9;
        }
    """,
    "parent_child_global_nested_array_reset": r"""
        struct B {unsigned a:3,b:5;};
        struct Box {struct B bits[2];int tail;};
        struct Box g={.bits[0].b=17,.bits[0]={.a=5},
                      .bits[1]={3,11},.bits[1].a=6,.tail=9};
        int main(void) {
            return g.bits[0].a!=5 || g.bits[0].b!=0
                || g.bits[1].a!=6 || g.bits[1].b!=11
                || g.tail!=9;
        }
    """,
    "global_bool_one_byte_and_normalization": r"""
        struct B {_Bool a:1,b:1;};
        struct B x={256,0};
        int main(void) {
            return x.a!=1 || x.b!=0 || sizeof(x)!=1;
        }
    """,
    "automatic_bool_one_byte_and_updates": r"""
        struct B {_Bool a:1,b:1;char tail;};
        int main(void) {
            struct B x={1,0,'Q'};
            x.b=7;
            x.a=0;
            x.a++;
            return x.a!=1 || x.b!=1 || x.tail!='Q'
                || sizeof(x)!=2;
        }
    """,
    "global_mixed_bool_int_bits": r"""
        struct B {_Bool a:1;unsigned int b:3;_Bool c:1;char tail;};
        struct B x={1,5,1,'Z'};
        int main(void) {
            return x.a!=1 || x.b!=5 || x.c!=1 || x.tail!='Z'
                || sizeof(x)!=4;
        }
    """,
    "automatic_mixed_bool_int_bits": r"""
        struct B {_Bool a:1;unsigned int b:3;_Bool c:1;char tail;};
        int main(void) {
            struct B x={.c=12,.b=5,.a=1,.tail='G'};
            x.b=6;
            x.c=0;
            x.c=20;
            return x.a!=1 || x.b!=6 || x.c!=1 || x.tail!='G'
                || sizeof(x)!=4;
        }
    """,
    "global_bool_after_nine_int_bits": r"""
        struct B {unsigned int a:9;_Bool b:1;unsigned int c:5;char tail;};
        struct B x={511,1,29,'Z'};
        int main(void) {
            return x.a!=511 || x.b!=1 || x.c!=29 || x.tail!='Z'
                || sizeof(x)!=4;
        }
    """,
    "automatic_bool_after_nine_int_bits": r"""
        struct B {unsigned int a:9;_Bool b:1;unsigned int c:5;char tail;};
        int main(void) {
            struct B x={.a=511,.b=0,.c=29,.tail='Y'};
            x.b=256;
            return x.a!=511 || x.b!=1 || x.c!=29 || x.tail!='Y'
                || sizeof(x)!=4;
        }
    """,
    "global_bool_zero_width_after_char": r"""
        struct B {char first;_Bool :0;_Bool a:1;char tail;};
        struct B x={'A',1,'B'};
        int main(void) {
            return x.first!='A' || x.a!=1 || x.tail!='B'
                || sizeof(x)!=3;
        }
    """,
    "automatic_bool_zero_width_after_char": r"""
        struct B {char first;unsigned int :0;_Bool a:1;char tail;};
        int main(void) {
            struct B x={'A',1,'B'};
            return x.first!='A' || x.a!=1 || x.tail!='B'
                || sizeof(x)!=6;
        }
    """,
    "union_bool_and_long": r"""
        union B {_Bool :0;_Bool a:1;long wide;};
        union B x={1};
        int main(void) {
            x.a=3;
            return x.a!=1 || sizeof(x)!=8;
        }
    """,
    "global_deep_bool_designators": r"""
        struct B {_Bool a:1,b:1;unsigned int c:3;};
        struct W {struct B nested[2];};
        struct W x={.nested[1].b=12,.nested[0].a=1};
        int main(void) {
            return x.nested[1].b!=1 || x.nested[0].a!=1
                || x.nested[0].b!=0 || x.nested[1].c!=0;
        }
    """,
    "automatic_bool_compound_literal": r"""
        struct B {_Bool a:1,b:1;};
        int main(void) {
            struct B *x=&(struct B){.a=256,.b=0};
            return x->a!=1 || x->b!=0 || sizeof(*x)!=1;
        }
    """,
    "global_unnamed_positional": r"""
        struct Bits {unsigned int a:3;unsigned int :2;unsigned int b:5;int tail;};
        struct Bits x={5,17,9};
        int main(void) {
            return x.a!=5 || x.b!=17 || x.tail!=9 || sizeof(x)!=8;
        }
    """,
    "automatic_unnamed_designated": r"""
        struct Bits {unsigned int a:3;unsigned int :2;unsigned int b:5;int tail;};
        int main(void) {
            struct Bits x={.tail=13,.b=17,.a=2};
            return x.a!=2 || x.b!=17 || x.tail!=13 || sizeof(x)!=8;
        }
    """,
    "global_zero_width_barrier": r"""
        struct Bits {unsigned int a:3;unsigned int :0;unsigned int b:5;int tail;};
        struct Bits x={3,7,99};
        int main(void) {
            return x.a!=3 || x.b!=7 || x.tail!=99 || sizeof(x)!=12;
        }
    """,
    "automatic_zero_width_after_char": r"""
        struct Bits {char a;unsigned int :0;char b;};
        int main(void) {
            struct Bits x={'A','B'};
            return x.a!='A' || x.b!='B' || sizeof(x)!=5;
        }
    """,
    "global_nonzero_unnamed_after_char": r"""
        struct Bits {char a;unsigned int :2;char b;};
        struct Bits x={'A','B'};
        int main(void) {
            return x.a!='A' || x.b!='B' || sizeof(x)!=3;
        }
    """,
    "union_unnamed_before_named": r"""
        union Bits {unsigned int :0;unsigned int a:3;long whole;};
        union Bits x={5};
        int main(void) {
            return x.a!=5 || sizeof(x)!=8;
        }
    """,
    "global_nested_deep_skips_unnamed": r"""
        struct Bits {unsigned int a:3;unsigned int :2;unsigned int b:5;};
        struct Wrap {int tag;struct Bits bits[2];};
        struct Wrap w={.bits[1].b=13,.bits[0].a=5,.tag=8};
        int main(void) {
            return w.tag!=8 || w.bits[0].a!=5 || w.bits[0].b!=0
                || w.bits[1].a!=0 || w.bits[1].b!=13;
        }
    """,
    "automatic_compound_literal_zero_width": r"""
        struct Bits {unsigned int a:3;unsigned int :0;unsigned int b:5;};
        int main(void) {
            struct Bits *p=&(struct Bits){.a=5,.b=7};
            return p->a!=5 || p->b!=7 || sizeof(*p)!=8;
        }
    """,
    "global_positional": r"""
        struct S {unsigned int a:3,b:5; signed int c:7;int tail;};
        struct S s={5,17,-11,9};
        int main(void) {
            return s.a!=5 || s.b!=17 || s.c!=-11
                || s.tail!=9 || sizeof(s)!=8;
        }
    """,
    "automatic_positional": r"""
        struct S {unsigned int a:3,b:5; signed int c:7;int tail;};
        int main(void) {
            struct S s={3,27,-7,123};
            return s.a!=3 || s.b!=27 || s.c!=-7
                || s.tail!=123 || sizeof(s)!=8;
        }
    """,
    "global_sparse_designation": r"""
        struct S {unsigned int a:3,b:5,c:12;};
        struct S s={.c=1599,.a=6};
        int main(void) {
            return s.a!=6 || s.b!=0 || s.c!=1599
                || sizeof(s)!=4;
        }
    """,
    "automatic_sparse_designation": r"""
        struct S {unsigned int a:3,b:5,c:12;};
        int main(void) {
            struct S s={.b=23,.a=3};
            return s.a!=3 || s.b!=23 || s.c!=0;
        }
    """,
    "global_member_after_char": r"""
        struct S {char marker;unsigned int a:3,b:5;int tail;};
        struct S s={.marker='Z',.a=5,.b=19,.tail=23};
        int main(void) {
            return s.marker!='Z' || s.a!=5 || s.b!=19
                || s.tail!=23 || sizeof(s)!=8;
        }
    """,
    "automatic_member_after_char": r"""
        struct S {char marker;unsigned int a:3,b:5;int tail;};
        int main(void) {
            struct S s={'A',4,28,99};
            return s.marker!='A' || s.a!=4 || s.b!=28
                || s.tail!=99 || sizeof(s)!=8;
        }
    """,
    "global_nested_braced": r"""
        struct Flags {unsigned int a:4,b:4;};
        struct Box {int id; struct Flags flags;int tail;};
        struct Box b={.flags={.b=11,.a=3},.id=9,.tail=31};
        int main(void) {
            return b.id!=9 || b.flags.a!=3 || b.flags.b!=11
                || b.tail!=31 || sizeof(b)!=12;
        }
    """,
    "automatic_nested_braced": r"""
        struct Flags {unsigned int a:4,b:4;};
        struct Box {int id; struct Flags flags;};
        int main(void) {
            struct Box b={5,{7,9}};
            return b.id!=5 || b.flags.a!=7 || b.flags.b!=9;
        }
    """,
    "global_deep_designated_fields": r"""
        struct Flags {unsigned int a:4,b:5;};
        struct Box {int id; struct Flags flags;};
        struct Box b={.flags.b=17,.id=11,.flags.a=9};
        int main(void) {
            return b.id!=11 || b.flags.a!=9 || b.flags.b!=17;
        }
    """,
    "automatic_deep_designated_fields": r"""
        struct Flags {unsigned int a:4,b:5;};
        struct Box {int id; struct Flags flags;};
        int main(void) {
            struct Box b={.flags.b=21,.flags.a=7,.id=13};
            return b.id!=13 || b.flags.a!=7 || b.flags.b!=21;
        }
    """,
    "global_array_deep_designators": r"""
        struct Bits {unsigned int a:3,b:5;};
        struct Bits xs[3]={[2].b=23,[0].a=7,[2].a=3};
        int main(void) {
            return xs[0].a!=7 || xs[0].b!=0 || xs[1].a!=0
                || xs[2].a!=3 || xs[2].b!=23;
        }
    """,
    "automatic_array_initializer": r"""
        struct Bits {unsigned int a:3,b:5;};
        int main(void) {
            struct Bits xs[2]={{3,17},{6,22}};
            return xs[0].a!=3 || xs[0].b!=17
                || xs[1].a!=6 || xs[1].b!=22;
        }
    """,
    "global_union_bitfield": r"""
        union Choice {unsigned int a:3; long value;};
        union Choice c={.a=5};
        int main(void) {return c.a!=5 || sizeof(c)!=8;}
    """,
    "automatic_union_bitfield": r"""
        union Choice {signed int a:5; long value;};
        int main(void) {
            union Choice c={.a=-11};
            return c.a!=-11 || sizeof(c)!=8;
        }
    """,
    "global_signed_and_full_width": r"""
        struct S {signed int a:5;unsigned int b:27;};
        struct S s={.a=-7,.b=1048577};
        struct Full {unsigned int word:32;};
        struct Full f={.word=0xABCD0123u};
        int main(void) {
            return s.a!=-7 || s.b!=1048577
                || f.word!=0xABCD0123u || sizeof(s)!=4;
        }
    """,
    "automatic_compound_literal": r"""
        struct S {unsigned int a:3,b:5;};
        int main(void) {
            struct S *p=&(struct S){.a=5,.b=13};
            return p->a!=5 || p->b!=13;
        }
    """,
    "static_compound_literal": r"""
        struct S {unsigned int a:3,b:5;};
        struct S *p=&(struct S){.a=3,.b=19};
        int main(void) {
            if(p->a!=3 || p->b!=19) return 1;
            p->a=6;
            return p->a!=6 || p->b!=19;
        }
    """,
    "global_nested_with_pointer": r"""
        struct Bits {unsigned int a:3,b:5;};
        struct Holder {struct Bits bits;int *ptr;};
        int external=41;
        struct Holder h={.bits={.a=3,.b=17},.ptr=&external};
        int main(void) {
            return h.bits.a!=3 || h.bits.b!=17 || *h.ptr!=41;
        }
    """,
    "initializers_then_runtime_updates": r"""
        struct Bits {unsigned int a:4,b:4;};
        struct Bits static_bits={.a=3,.b=13};
        int main(void) {
            struct Bits automatic={.a=2,.b=7};
            static_bits.a++;
            automatic.b+=3;
            return static_bits.a!=4 || static_bits.b!=13
                || automatic.a!=2 || automatic.b!=10;
        }
    """,
    "global_word_boundary": r"""
        struct Bits {unsigned int first:30,second:4,third:5;};
        struct Bits bits={.first=123456789,.second=11,.third=23};
        int main(void) {
            return bits.first!=123456789 || bits.second!=11
                || bits.third!=23 || sizeof(bits)!=8;
        }
    """,
}


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, text=True, capture_output=True,
                          check=False, timeout=60)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path,
                        default=ROOT / "target/debug/mini-c-cranelift")
    args = parser.parse_args()
    backend = args.backend.resolve()
    with tempfile.TemporaryDirectory(prefix="mini-c-bitfield-init-") as dirname:
        base = Path(dirname)
        for name, source in CASES.items():
            src = base / f"{name}.c"
            obj = base / f"{name}.o"
            gcc = base / f"{name}.gcc"
            mini = base / f"{name}.mini"
            src.write_text(source, encoding="utf-8")
            for command in (
                ["cc", "-std=c99", "-pedantic-errors", "-O0",
                 "-Wno-override-init", "-Wno-override-init-side-effects",
                 str(src), "-o", str(gcc)],
                [str(backend), "--emit-obj", str(obj), str(src)],
                ["cc", str(obj), "-o", str(mini)],
                [str(gcc)],
                [str(mini)],
            ):
                result = run(command)
                if result.returncode != 0 or result.stdout or result.stderr:
                    raise AssertionError(
                        f"{name}: {' '.join(command)} code={result.returncode}; "
                        f"stdout={result.stdout!r} stderr={result.stderr!r}"
                    )
            if not obj.read_bytes().startswith(b"\x7fELF"):
                raise AssertionError(f"{name}: expected native ELF")
            print(f"{name}: GCC/Mini-C bit-field initializer execution PASS")
    print(f"Bit-field C99 aggregate initializer native suite: {len(CASES)} cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
