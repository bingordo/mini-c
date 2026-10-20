//! Target-dependent C integer types and the conversions used by the supported subset.
//! No host sizeof() assumptions: a caller selects the integer model.
use mini_c_syntax::ast::{Type, Span};
use mini_c_syntax::diag::Diagnostic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntRank { Char, Int, Long }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CType {
    Void,
    Integer { rank: IntRank, unsigned: bool },
    Pointer(Box<CType>),
    Function { result: Box<CType>, params: Vec<CType> },
}

impl CType {
    pub fn int() -> Self { Self::Integer { rank: IntRank::Int, unsigned: false } }
    pub fn is_integer(&self) -> bool { matches!(self, Self::Integer { .. }) }
    pub fn is_scalar(&self) -> bool { self.is_integer() || matches!(self, Self::Pointer(_)) }
    pub fn is_object(&self) -> bool { !matches!(self, Self::Void | Self::Function { .. }) }
}

#[derive(Debug, Clone, Copy)]
pub struct Target {
    pub char_is_signed: bool,
    pub int_bits: u16,
    pub long_bits: u16,
}

impl Default for Target {
    fn default() -> Self { Self { char_is_signed: true, int_bits: 32, long_bits: 64 } }
}

impl Target {
    pub fn validate(self) -> Result<Self, Diagnostic> {
        if !(16..=64).contains(&self.int_bits) || !(self.int_bits..=64).contains(&self.long_bits) {
            return Err(Diagnostic::new(Span::default(), "invalid target: expected 16 <= int_bits <= long_bits <= 64"));
        }
        Ok(self)
    }

    pub fn bits(self, rank: IntRank) -> u16 {
        match rank { IntRank::Char => 8, IntRank::Int => self.int_bits, IntRank::Long => self.long_bits }
    }

    pub fn promote(self, ty: &CType) -> CType {
        match ty {
            CType::Integer { rank: IntRank::Char, .. } => CType::int(),
            _ => ty.clone(),
        }
    }

    /// ISO C usual arithmetic conversions for char/int/long, signed and unsigned.
    pub fn common_integer(self, lhs: &CType, rhs: &CType) -> Option<CType> {
        let lhs = self.promote(lhs);
        let rhs = self.promote(rhs);
        let (CType::Integer { rank: a, unsigned: au }, CType::Integer { rank: b, unsigned: bu }) = (&lhs, &rhs) else {
            return None;
        };
        let (a, au, b, bu) = (*a, *au, *b, *bu);
        if au == bu {
            return Some(CType::Integer { rank: a.max(b), unsigned: au });
        }
        let (ur, sr) = if au { (a, b) } else { (b, a) };
        let (rank, unsigned) = if ur >= sr {
            (ur, true)
        } else if self.bits(sr) > self.bits(ur) {
            (sr, false)
        } else {
            (sr, true)
        };
        Some(CType::Integer { rank, unsigned })
    }

    pub fn decimal_literal(self, value: u64) -> Option<CType> {
        for rank in [IntRank::Int, IntRank::Long] {
            let bits = self.bits(rank);
            let maximum = (1u128 << (bits - 1)) - 1;
            if u128::from(value) <= maximum {
                return Some(CType::Integer { rank, unsigned: false });
            }
        }
        None
    }
}

pub fn type_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usual_arithmetic_conversions() {
        let t = Target::default();
        let int = CType::int();
        let uint = CType::Integer { rank: IntRank::Int, unsigned: true };
        let long = CType::Integer { rank: IntRank::Long, unsigned: false };
        let ulong = CType::Integer { rank: IntRank::Long, unsigned: true };
        let char_ty = CType::Integer { rank: IntRank::Char, unsigned: true };
        assert_eq!(t.common_integer(&int, &char_ty), Some(int.clone()));
        assert_eq!(t.common_integer(&int, &uint), Some(uint.clone()));
        assert_eq!(t.common_integer(&long, &uint), Some(long.clone()));
        assert_eq!(t.common_integer(&long, &ulong), Some(ulong));
        let narrow = Target { int_bits: 32, long_bits: 32, ..t };
        assert_eq!(narrow.common_integer(&long, &uint), Some(CType::Integer { rank: IntRank::Long, unsigned: true }));
    }

    #[test]
    fn decimal_literal_selection() {
        let t = Target::default();
        assert_eq!(t.decimal_literal(2_147_483_647), Some(CType::int()));
        assert_eq!(t.decimal_literal(2_147_483_648), Some(CType::Integer { rank: IntRank::Long, unsigned: false }));
        assert_eq!(t.decimal_literal(u64::MAX), None);
        let small = Target { int_bits: 16, long_bits: 32, ..t };
        assert_eq!(small.decimal_literal(40000), Some(CType::Integer { rank: IntRank::Long, unsigned: false }));
    }

    #[test]
    fn invalid_target_is_diagnostic() {
        assert!(Target { int_bits: 8, ..Target::default() }.validate().is_err());
    }
}
