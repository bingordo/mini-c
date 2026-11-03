//! Deterministic, defined-behavior C generation with a separate numeric oracle.
//! The oracle operates on generated arithmetic instructions, never on Mini-C AST.
use mini_c_syntax::parser::Parser;
use mini_c_syntax::stream::ItemSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op { Add, Sub, Mul, Div, Rem, BitAnd, BitOr, BitXor, Shl, Shr }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction { pub op: Op, pub rhs: i32 }

#[derive(Debug, Clone)]
pub struct Case { pub seed: u64, pub instructions: Vec<Instruction>, pub expected: i32, pub source: String }

#[derive(Debug, Clone)]
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn range(&mut self, n: u64) -> u64 { self.next() % n }
}

/// Returns None for any operation that would have undefined signed arithmetic.
pub fn checked_step(lhs: i32, ins: Instruction) -> Option<i32> {
    match ins.op {
        Op::Add => lhs.checked_add(ins.rhs),
        Op::Sub => lhs.checked_sub(ins.rhs),
        Op::Mul => lhs.checked_mul(ins.rhs),
        Op::Div => lhs.checked_div(ins.rhs),
        Op::Rem => lhs.checked_rem(ins.rhs),
        // Keep bitwise operations on nonnegative signed operands, so their
        // semantics do not depend on representations of negative integers.
        Op::BitAnd if lhs >= 0 && ins.rhs >= 0 => Some(lhs & ins.rhs),
        Op::BitOr if lhs >= 0 && ins.rhs >= 0 => Some(lhs | ins.rhs),
        Op::BitXor if lhs >= 0 && ins.rhs >= 0 => Some(lhs ^ ins.rhs),
        // C's signed left-shift is only defined if the result is representable;
        // the generator subsequently limits every result to [-32767, 32767].
        Op::Shl if lhs >= 0 && (0..=4).contains(&ins.rhs) =>
            lhs.checked_shl(ins.rhs as u32),
        Op::Shr if lhs >= 0 && (0..=4).contains(&ins.rhs) =>
            lhs.checked_shr(ins.rhs as u32),
        _ => None,
    }
}

fn symbol(op: Op) -> &'static str {
    match op {
        Op::Add => "+", Op::Sub => "-", Op::Mul => "*", Op::Div => "/", Op::Rem => "%",
        Op::BitAnd => "&", Op::BitOr => "|", Op::BitXor => "^",
        Op::Shl => "<<", Op::Shr => ">>",
    }
}

/// Generate a side-effecting pointer/index exercise with a numerical oracle.
/// All pointer values remain within the same array, and the index changes once.
fn emit_pointer_update_case(source: &mut String, rng: &mut Rng) {
    let first = 10 + rng.range(50) as i32;
    let second = first + 7;
    let third = second + 9;
    let fourth = third + 11;
    source.push_str(&format!(
        "{{ int data[4]; data[0]={first}; data[1]={second}; \
         data[2]={third}; data[3]={fourth}; \
         int *p=data; int *old=p++; \
         if (old!=data || p!=data+1) return 0; \
         int *newp=++p; \
         if (newp!=data+2 || *newp!={third}) return 0; \
         int *prev=p--; \
         if (prev!=data+2 || p!=data+1) return 0; \
         int *firstp=--p; \
         if (firstp!=data || p!=data) return 0; \
         int k=0; int oldvalue=data[k++]++; \
         if (k!=1 || oldvalue!={first} || data[0]!={}) return 0; \
         int prefixvalue=++data[k]; \
         if (prefixvalue!={} || data[1]!={}) return 0; }}\n",
        first + 1, second + 1, second + 1,
    ));
}

/// Independent oracle: checked i32 operations on a compact instruction stream.
/// Generated values stay within the portable C int range [-32767, 32767].
pub fn generate(seed: u64, steps: usize) -> Case {
    let mut rng = Rng(seed);
    let mut value = 1i32;
    let mut instructions = Vec::with_capacity(steps);
    let mut source = String::from("int main(void) { int x = 1;\n");
    for iteration in 0..steps {
        let op = match rng.range(10) {
            0 => Op::Add, 1 => Op::Sub, 2 => Op::Mul,
            3 => Op::Div, 4 => Op::Rem, 5 => Op::BitAnd,
            6 => Op::BitOr, 7 => Op::BitXor, 8 => Op::Shl,
            _ => Op::Shr,
        };
        let rhs = match op {
            Op::Shl | Op::Shr => rng.range(5) as i32,
            Op::BitAnd | Op::BitOr | Op::BitXor => rng.range(256) as i32,
            _ => (rng.range(41) as i32) + 1,
        };
        let ins = Instruction { op, rhs };
        if let Some(next) = checked_step(value, ins).filter(|v| (-32767..=32767).contains(v)) {
            // Every generated operator has a compound-assignment form.
            // Randomize spelling only AFTER checked_step established defined
            // C behavior, so both forms use the identical numeric oracle.
            let compound = rng.range(2) == 0;
            if compound {
                source.push_str(&format!("x {}= {};\n", symbol(op), rhs));
            } else {
                source.push_str(&format!("x = x {} {};\n", symbol(op), rhs));
            }
            value = next;
            instructions.push(ins);
        }

        // Check the *value* of the update expression independently of its
        // effect on x. This catches wrong prefix/postfix result selection even
        // when the final x happens to match GCC.
        if iteration % 7 == 6 {
            let increment = rng.range(2) == 0;
            let postfix = rng.range(2) == 0;
            let ins = Instruction {
                op: if increment { Op::Add } else { Op::Sub }, rhs: 1,
            };
            if let Some(next) = checked_step(value, ins)
                .filter(|v| (-32767..=32767).contains(v))
            {
                let observed = if postfix { value } else { next };
                let token = if increment { "++" } else { "--" };
                let update = if postfix {
                    format!("x{token}")
                } else {
                    format!("{token}x")
                };
                source.push_str(&format!(
                    "{{ int observed = {update}; if (observed != {observed}) return 0; }}\n"
                ));
                value = next;
                instructions.push(ins);
            }
        }

        // Conditional that never leaves the portable C int interval.
        // The recorded trace contains the *executed* branch only.
        if iteration % 13 == 12 {
            let chosen = if value >= 0 {
                Instruction { op: Op::Sub, rhs: 1 }
            } else {
                Instruction { op: Op::Add, rhs: 1 }
            };
            value = checked_step(value, chosen).expect("guarded conditional arithmetic");
            instructions.push(chosen);
            source.push_str(
                "if (x >= 0) { x = x - 1; } else { x = x + 1; }\n"
            );
        }

        // Every iteration is bounded, and /2 cannot overflow or divide by zero.
        // The block scope allows multiple loops without redeclaring one name.
        if iteration % 29 == 28 {
            let repetitions = (rng.range(4) + 1) as usize;
            source.push_str(&format!(
                "{{ int remaining = {repetitions}; while (remaining > 0) {{ \
                 x = x / 2; remaining = remaining - 1; }} }}\n"
            ));
            for _ in 0..repetitions {
                let division = Instruction { op: Op::Div, rhs: 2 };
                value = checked_step(value, division).expect("division by two");
                instructions.push(division);
            }
        }
    }
    if steps != 0 {
        emit_pointer_update_case(&mut source, &mut rng);
    }
    source.push_str(&format!("return x == {};\n}}\n", value));
    Case { seed, instructions, expected: value, source }
}

/// Test generated syntax through the public streaming parser, not through the oracle.
pub fn parse_case(case: &Case) -> Result<(), String> {
    let mut parser = Parser::new(&case.source);
    while parser.next_item().map_err(|e| e.message)?.is_some() {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_seed() {
        let a = generate(42, 80);
        let b = generate(42, 80);
        assert_eq!(a.source, b.source);
        assert_eq!(a.expected, b.expected);
        assert_ne!(a.source, generate(43, 80).source);
    }
    #[test]
    fn arithmetic_guards() {
        assert_eq!(checked_step(i32::MAX, Instruction { op: Op::Add, rhs: 1 }), None);
        assert_eq!(checked_step(1, Instruction { op: Op::Div, rhs: 0 }), None);
        assert_eq!(checked_step(i32::MIN, Instruction { op: Op::Div, rhs: -1 }), None);
        assert_eq!(checked_step(1, Instruction { op: Op::Rem, rhs: 0 }), None);
    }
    #[test]
    fn recorded_trace_is_bounded_and_matches_oracle() {
        for seed in [0, 1, 7, 42, 999, u64::MAX] {
            let case = generate(seed, 250);
            let mut value = 1i32;
            for instruction in &case.instructions {
                value = checked_step(value, *instruction).expect("valid generated arithmetic");
                assert!((-32767..=32767).contains(&value), "seed {seed}: {value}");
            }
            assert_eq!(value, case.expected, "seed {seed}");
            assert!(case.source.contains("if (x >= 0)"));
            assert!(case.source.contains("while (remaining > 0)"));
        }
    }

    #[test]
    fn bitwise_shift_oracle_guards_defined_signed_operations() {
        let instruction = |op, rhs| Instruction { op, rhs };
        assert_eq!(checked_step(10, instruction(Op::BitAnd, 6)), Some(2));
        assert_eq!(checked_step(10, instruction(Op::BitOr, 6)), Some(14));
        assert_eq!(checked_step(10, instruction(Op::BitXor, 6)), Some(12));
        assert_eq!(checked_step(10, instruction(Op::Shl, 2)), Some(40));
        assert_eq!(checked_step(40, instruction(Op::Shr, 2)), Some(10));
        assert_eq!(checked_step(-1, instruction(Op::Shl, 2)), None);
        assert_eq!(checked_step(-1, instruction(Op::Shr, 2)), None);
        assert_eq!(checked_step(1, instruction(Op::Shl, 32)), None);
        assert_eq!(checked_step(1, instruction(Op::Shr, -1)), None);
    }

    #[test]
    fn seeded_programs_cover_binary_and_compound_bitwise_shifts() {
        let mut op_seen = [false; 5];
        let mut compound_seen = [false; 5];
        for seed in 0..64 {
            let case = generate(seed, 120);
            parse_case(&case).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            for (i, op) in [Op::BitAnd, Op::BitOr, Op::BitXor, Op::Shl, Op::Shr].iter().enumerate() {
                op_seen[i] |= case.instructions.iter().any(|ins| ins.op == *op);
                compound_seen[i] |= case.source.contains(&format!("x {}= ", symbol(*op)));
            }
        }
        assert!(op_seen.into_iter().all(|seen| seen), "missing generated bitwise or shift operation");
        assert!(compound_seen.into_iter().all(|seen| seen), "missing compound-assignment coverage");
    }

    #[test]
    fn seeded_arithmetic_compound_assignments_cover_every_operator() {
        let mut binary_seen = [false; 5];
        let mut compound_seen = [false; 5];
        for seed in 0..96 {
            let case = generate(seed, 150);
            parse_case(&case).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            for (i, op) in [Op::Add, Op::Sub, Op::Mul, Op::Div, Op::Rem].iter().enumerate() {
                let sym = symbol(*op);
                binary_seen[i] |= case.source.contains(&format!("x = x {sym} "));
                compound_seen[i] |= case.source.contains(&format!("x {sym}= "));
            }
        }
        assert!(binary_seen.into_iter().all(|seen| seen),
            "missing generated ordinary arithmetic operator");
        assert!(compound_seen.into_iter().all(|seen| seen),
            "missing generated arithmetic compound operator");
    }

    #[test]
    fn seeded_updates_cover_old_new_results_and_pointer_side_effects() {
        let mut prefix_increment = false;
        let mut postfix_increment = false;
        let mut prefix_decrement = false;
        let mut postfix_decrement = false;
        for seed in 0..96 {
            let case = generate(seed, 140);
            parse_case(&case).unwrap_or_else(|err| panic!("seed {seed}: {err}"));
            prefix_increment |= case.source.contains("observed = ++x;");
            postfix_increment |= case.source.contains("observed = x++;");
            prefix_decrement |= case.source.contains("observed = --x;");
            postfix_decrement |= case.source.contains("observed = x--;");
            assert!(case.source.contains("old=p++"));
            assert!(case.source.contains("newp=++p"));
            assert!(case.source.contains("prev=p--"));
            assert!(case.source.contains("firstp=--p"));
            assert!(case.source.contains("oldvalue=data[k++]++"));
            assert!(case.source.contains("prefixvalue=++data[k]"));
            let mut value = 1;
            for instruction in &case.instructions {
                value = checked_step(value, *instruction)
                    .expect("generator emitted a defined update");
            }
            assert_eq!(value, case.expected, "seed {seed}");
        }
        assert!(prefix_increment && postfix_increment
            && prefix_decrement && postfix_decrement,
            "not all four update forms occurred across seeds");
    }

    #[test]
    fn zero_step_case_is_valid() {
        let case = generate(0, 0);
        assert!(case.instructions.is_empty());
        assert_eq!(case.expected, 1);
        parse_case(&case).unwrap();
    }

    #[test]
    fn seeded_parser_smoke() {
        for seed in 0..64 {
            let case = generate(seed, 100);
            parse_case(&case).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        }
    }
}
