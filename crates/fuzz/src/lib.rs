//! Deterministic, defined-behavior C generation with a separate numeric oracle.
//! The oracle operates on generated arithmetic instructions, never on Mini-C AST.
use mini_c_syntax::parser::Parser;
use mini_c_syntax::stream::ItemSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op { Add, Sub, Mul, Div, Rem, Xor }

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
        Op::Xor => Some(lhs ^ ins.rhs),
    }
}

fn symbol(op: Op) -> &'static str {
    match op { Op::Add => "+", Op::Sub => "-", Op::Mul => "*", Op::Div => "/", Op::Rem => "%", Op::Xor => "^" }
}

/// Independent oracle: checked i32 operations on a compact instruction stream.
/// Generated values stay within the portable C int range [-32767, 32767].
pub fn generate(seed: u64, steps: usize) -> Case {
    let mut rng = Rng(seed);
    let mut value = 1i32;
    let mut instructions = Vec::with_capacity(steps);
    let mut source = String::from("int main(void) { int x = 1;\\n");
    for _ in 0..steps {
        let op = match rng.range(6) {
            0 => Op::Add, 1 => Op::Sub, 2 => Op::Mul,
            3 => Op::Div, 4 => Op::Rem, _ => Op::Xor,
        };
        let rhs = (rng.range(41) as i32) + 1;
        let ins = Instruction { op, rhs };
        if let Some(next) = checked_step(value, ins).filter(|v| (-32767..=32767).contains(v)) {
            source.push_str(&format!("x = x {} {};\\n", symbol(op), rhs));
            value = next;
            instructions.push(ins);
        }
    }
    source.push_str(&format!("return x == {};\\n}}\\n", value));
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
    fn seeded_parser_smoke() {
        for seed in 0..64 {
            let case = generate(seed, 100);
            parse_case(&case).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        }
    }
}
