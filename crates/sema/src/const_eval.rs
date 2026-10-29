//! Evaluate checked integer constant expressions using the caller's target model.
//! This is for initializer constraints, not runtime evaluation or optimization.
use crate::types::{CType, Target};
use crate::{CheckedExpr, CheckedExprKind};
use mini_c_syntax::ast::{BinOp, UnOp};

fn normalize(value: i128, ty: &CType, target: Target) -> Result<i128, &'static str> {
    let CType::Integer { rank, unsigned } = ty else {
        return Err("not an integer constant expression");
    };
    let bits = target.bits(*rank);
    let modulus = 1i128 << bits;
    let upper = if *unsigned { modulus - 1 } else { (modulus >> 1) - 1 };
    let lower = if *unsigned { 0 } else { -(modulus >> 1) };
    if (lower..=upper).contains(&value) {
        Ok(value)
    } else if *unsigned {
        Ok(value.rem_euclid(modulus))
    } else {
        // Conversion to a signed type outside its range is implementation
        // defined. Model the selected target's two's-complement truncation.
        let v = value.rem_euclid(modulus);
        Ok(if v > upper { v - modulus } else { v })
    }
}

fn bounded_arithmetic(value: i128, ty: &CType, target: Target) -> Result<i128, &'static str> {
    let CType::Integer { rank, unsigned } = ty else {
        return Err("arithmetic requires an integer type");
    };
    if !*unsigned {
        let bits = target.bits(*rank);
        let bound = 1i128 << (bits - 1);
        if !(-bound..bound).contains(&value) {
            return Err("signed integer overflow in constant expression");
        }
    }
    normalize(value, ty, target)
}

pub fn evaluate(expr: &CheckedExpr, target: Target) -> Result<i128, &'static str> {
    match &expr.kind {
        CheckedExprKind::Literal(value) => Ok(i128::from(*value)),
        CheckedExprKind::Convert(inner) => normalize(evaluate(inner, target)?, &expr.ty, target),
        CheckedExprKind::Unary(op, inner) => {
            let value = evaluate(inner, target)?;
            match op {
                UnOp::Plus => Ok(value),
                UnOp::Neg => {
                    let negated = value.checked_neg().ok_or("signed integer overflow in constant expression")?;
                    bounded_arithmetic(negated, &expr.ty, target)
                }
                UnOp::Not => Ok(i128::from(value == 0)),
                UnOp::BitNot => normalize(!value, &expr.ty, target),
                UnOp::Deref | UnOp::Address => Err("not an integer constant expression"),
            }
        }
        CheckedExprKind::Binary(op, lhs, rhs) => {
            let a = evaluate(lhs, target)?;
            // Respect C short-circuit semantics even in constant expressions.
            match op {
                BinOp::And if a == 0 => return Ok(0),
                BinOp::Or if a != 0 => return Ok(1),
                _ => {}
            }
            let b = evaluate(rhs, target)?;
            let result = match op {
                BinOp::Add => a.checked_add(b),
                BinOp::Sub => a.checked_sub(b),
                BinOp::Mul => a.checked_mul(b),
                BinOp::Div => a.checked_div(b),
                BinOp::Mod => a.checked_rem(b),
                BinOp::Eq => return Ok(i128::from(a == b)),
                BinOp::Ne => return Ok(i128::from(a != b)),
                BinOp::Lt => return Ok(i128::from(a < b)),
                BinOp::Le => return Ok(i128::from(a <= b)),
                BinOp::Gt => return Ok(i128::from(a > b)),
                BinOp::Ge => return Ok(i128::from(a >= b)),
                BinOp::BitAnd => return normalize(a & b, &expr.ty, target),
                BinOp::BitOr => return normalize(a | b, &expr.ty, target),
                BinOp::BitXor => return normalize(a ^ b, &expr.ty, target),
                BinOp::And => return Ok(i128::from(a != 0 && b != 0)),
                BinOp::Or => return Ok(i128::from(a != 0 || b != 0)),
            };
            let result = result.ok_or("integer constant expression divides by zero or overflows")?;
            bounded_arithmetic(result, &expr.ty, target)
        }
        CheckedExprKind::Name(_) | CheckedExprKind::Load(_) |
        CheckedExprKind::ArrayDecay(_) | CheckedExprKind::Assign(..) | CheckedExprKind::Call(..) =>
            Err("not an integer constant expression"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Category, CheckedExprKind as K, CheckedExpr as E};
    use crate::types::IntRank;
    fn lit(n: u64) -> E {
        E { ty: CType::int(), category: Category::Value, kind: K::Literal(n) }
    }
    #[test]
    fn signed_overflow_and_division_by_zero_are_rejected() {
        let t = Target::default();
        let expr = E::value(CType::int(), K::Binary(BinOp::Add, Box::new(lit(2147483647)), Box::new(lit(1))));
        assert!(evaluate(&expr, t).unwrap_err().contains("overflow"));
        let expr = E::value(CType::int(), K::Binary(BinOp::Div, Box::new(lit(3)), Box::new(lit(0))));
        assert!(evaluate(&expr, t).is_err());
    }
    #[test]
    fn unsigned_wrap_and_short_circuit() {
        let t = Target::default();
        let unsigned = CType::Integer { rank: IntRank::Int, unsigned: true };
        let a = E::value(unsigned.clone(), K::Convert(Box::new(lit(0))));
        let b = E::value(unsigned.clone(), K::Convert(Box::new(lit(1))));
        let sub = E::value(unsigned, K::Binary(BinOp::Sub, Box::new(a), Box::new(b)));
        assert_eq!(evaluate(&sub, t), Ok(u32::MAX as i128));
        let div = E::value(CType::int(), K::Binary(BinOp::Div, Box::new(lit(1)), Box::new(lit(0))));
        let and = E::value(CType::int(), K::Binary(BinOp::And, Box::new(lit(0)), Box::new(div)));
        assert_eq!(evaluate(&and, t), Ok(0));
    }
}
