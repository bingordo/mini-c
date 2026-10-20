//! Deterministic, defined-behavior C generation with a separate numeric oracle.
//! The oracle operates on generated arithmetic instructions, never on Mini-C AST.
use mini_c_syntax::parser::Parser;
use mini_c_syntax::stream::ItemSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op { Add, Sub, Mul, Div, Rem }

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
    }
}

fn symbol(op: Op) -> &'static str {
    match op { Op::Add => "+", Op::Sub => "-", Op::Mul => "*", Op::Div => "/", Op::Rem => "%" }
}

/// Independent oracle: checked i32 operations on a compact instruction stream.
/// Generated values stay within the portable C int range [-32767, 32767].
pub fn generate(seed: u64, steps: usize) -> Case {
    let mut rng = Rng(seed);
    let mut value = 1i32;
    let mut instructions = Vec::with_capacity(steps);
    let mut source = String::from("int main(void) { int x = 1;\n");
    for iteration in 0..steps {
        let op = match rng.range(5) {
            0 => Op::Add, 1 => Op::Sub, 2 => Op::Mul,
            3 => Op::Div, _ => Op::Rem,
        };
        let rhs = (rng.range(41) as i32) + 1;
        let ins = Instruction { op, rhs };
        if let Some(next) = checked_step(value, ins).filter(|v| (-32767..=32767).contains(v)) {
            source.push_str(&format!("x = x {} {};\n", symbol(op), rhs));
            value = next;
            instructions.push(ins);
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
