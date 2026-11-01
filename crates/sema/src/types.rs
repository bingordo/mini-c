//! Target-dependent C integer types and the conversions used by the supported subset.
//! No host sizeof() assumptions: a caller selects the integer model.
use mini_c_syntax::ast::{Span, TagKind};
use mini_c_syntax::diag::Diagnostic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntRank { Char, Int, Long }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructMember {
    pub name: String,
    pub ty: CType,
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CType {
    Struct {
        /// Nominal identity; distinct inline definitions are distinct C types.
        identity: u64,
        tag: Option<String>,
        members: Vec<StructMember>,
        size: u64,
        align: u64,
    },
    Union {
        identity: u64,
        tag: Option<String>,
        members: Vec<StructMember>,
        size: u64,
        align: u64,
    },
    /// Nominal reference to a named, possibly incomplete struct/union.
    /// Pointers retain identity while the tag's definition can arrive later.
    TagRef { kind: TagKind, name: String, identity: u64 },
    Void,
    Integer { rank: IntRank, unsigned: bool },
    Pointer(Box<CType>),
    /// Incomplete arrays use None; bound and full object layout are target-dependent.
    Array(Box<CType>, Option<u64>),
    Function { result: Box<CType>, params: Vec<CType> },
}

impl CType {
    /// Target-specific object alignment in bytes for x86-64 LP64.
    pub fn align_bytes(&self, target: Target) -> Option<u64> {
        match self {
            Self::Void | Self::Function { .. } | Self::TagRef { .. } => None,
            Self::Struct { align, .. } | Self::Union { align, .. } => Some(*align),
            Self::Array(element, _) => element.align_bytes(target),
            Self::Pointer(_) => Some(8),
            Self::Integer { rank, .. } => Some(u64::from(target.bits(*rank) / 8)),
        }
    }

    /// A named aggregate and its recursive/incomplete reference share
    /// identity even though one representation does not embed the members.
    pub fn same_type(&self, other: &Self) -> bool {
        use CType::{Struct, Union, TagRef};
        match (self, other) {
            (Struct { identity: a, .. }, Struct { identity: b, .. }) |
            (Union { identity: a, .. }, Union { identity: b, .. }) =>
                a == b,
            (Struct { identity: a, .. }, TagRef { kind: TagKind::Struct, identity: b, .. }) |
            (TagRef { kind: TagKind::Struct, identity: a, .. }, Struct { identity: b, .. }) |
            (Union { identity: a, .. }, TagRef { kind: TagKind::Union, identity: b, .. }) |
            (TagRef { kind: TagKind::Union, identity: a, .. }, Union { identity: b, .. }) =>
                a == b,
            (CType::Pointer(a), CType::Pointer(b)) => a.same_type(b),
            (CType::Array(a, na), CType::Array(b, nb)) =>
                na == nb && a.same_type(b),
            _ => self == other,
        }
    }

    pub fn int() -> Self { Self::Integer { rank: IntRank::Int, unsigned: false } }
    /// Composite type for compatible global declarations. An incomplete array
    /// declaration may be completed by another declaration in the same TU.
    /// Different fixed bounds and different element types remain incompatible.
    pub fn composite(&self, other: &Self) -> Option<Self> {
        if self == other { return Some(self.clone()); }
        match (self, other) {
            (Self::Pointer(a), Self::Pointer(b)) =>
                Some(Self::Pointer(Box::new(a.composite(b)?))),
            (Self::Array(a, na), Self::Array(b, nb)) => {
                let count = match (na, nb) {
                    (Some(x), Some(y)) if x != y => return None,
                    (Some(x), _) | (_, Some(x)) => Some(*x),
                    (None, None) => None,
                };
                Some(Self::Array(Box::new(a.composite(b)?), count))
            }
            (Self::Function { result: ar, params: ap },
             Self::Function { result: br, params: bp }) if ap.len() == bp.len() => {
                let result = Box::new(ar.composite(br)?);
                let params = ap.iter().zip(bp).map(|(a,b)| a.composite(b))
                    .collect::<Option<Vec<_>>>()?;
                Some(Self::Function { result, params })
            }
            _ if self.same_type(other) => {
                Some(if self.size_bytes(Target::default()).is_some() {
                    self.clone()
                } else { other.clone() })
            }
            _ => None,
        }
    }

    pub fn is_integer(&self) -> bool { matches!(self, Self::Integer { .. }) }
    pub fn is_scalar(&self) -> bool { self.is_integer() || matches!(self, Self::Pointer(_)) }
    pub fn is_object(&self) -> bool { !matches!(self, Self::Void | Self::Function { .. }) }

    /// Known object size in bytes for the currently supported LP64 model.
    /// Incomplete arrays, void/function types and overflowing extents have no size.
    pub fn size_bytes(&self, target: Target) -> Option<u64> {
        match self {
            Self::Void | Self::Function { .. } | Self::TagRef { .. } => None,
            Self::Struct { size, .. } | Self::Union { size, .. } => Some(*size),
            Self::Integer { rank, .. } => Some(u64::from(target.bits(*rank) / 8)),
            Self::Pointer(_) => Some(8), // Current target model is LP64.
            Self::Array(element, Some(count)) => element.size_bytes(target)?.checked_mul(*count),
            Self::Array(_, None) => None,
        }
    }
}

/// Round up without overflowing the target's size representation.
pub fn align_up(value: u64, alignment: u64) -> Option<u64> {
    let mask = alignment.checked_sub(1)?;
    if alignment == 0 || !alignment.is_power_of_two() { return None; }
    value.checked_add(mask).map(|v| v & !mask)
}

/// Resolve ordered structure members using the selected target's alignment.
/// No packing, bitfields, or flexible-array members are supported yet.
pub fn layout_struct(
    tag: Option<String>, fields: Vec<(String, CType)>, target: Target,
) -> Result<CType, &'static str> {
    if fields.is_empty() { return Err("empty structures are unsupported"); }
    let mut size = 0u64;
    let mut align = 1u64;
    let mut members = Vec::with_capacity(fields.len());
    let mut seen = std::collections::HashSet::new();
    for (name, ty) in fields {
        if !seen.insert(name.clone()) { return Err("duplicate structure member"); }
        let field_size = ty.size_bytes(target).ok_or("structure member has incomplete type")?;
        let field_align = ty.align_bytes(target).ok_or("structure member has incomplete type")?;
        if field_size == 0 || !ty.is_object() {
            return Err("structure member must have a complete object type");
        }
        align = align.max(field_align);
        let offset = align_up(size, field_align).ok_or("structure member layout overflow")?;
        size = offset.checked_add(field_size).ok_or("structure size overflow")?;
        members.push(StructMember { name, ty, offset });
    }
    size = align_up(size, align).ok_or("structure size overflow")?;
    Ok(CType::Struct { identity: 0, tag, members, size, align })
}

/// C union members all start at byte zero and share the maximum size
/// rounded to the strictest member alignment.
pub fn layout_union(
    tag: Option<String>, fields: Vec<(String, CType)>, target: Target,
) -> Result<CType, &'static str> {
    if fields.is_empty() { return Err("empty unions are unsupported"); }
    let mut size = 0u64;
    let mut align = 1u64;
    let mut members = Vec::with_capacity(fields.len());
    let mut seen = std::collections::HashSet::new();
    for (name, ty) in fields {
        if !seen.insert(name.clone()) { return Err("duplicate union member"); }
        let field_size = ty.size_bytes(target).ok_or("union member has incomplete type")?;
        let field_align = ty.align_bytes(target).ok_or("union member has incomplete type")?;
        if field_size == 0 || !ty.is_object() {
            return Err("union member must have a complete object type");
        }
        align = align.max(field_align);
        size = size.max(field_size);
        members.push(StructMember { name, ty, offset: 0 });
    }
    size = align_up(size, align).ok_or("union size overflow")?;
    Ok(CType::Union { identity: 0, tag, members, size, align })
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
            return Err(Diagnostic::semantic(Span::default(), "invalid target: expected 16 <= int_bits <= long_bits <= 64"));
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
    Diagnostic::semantic(span, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn union_overlay_layout_and_nested_alignment() {
        let union = layout_union(Some("Word".into()), vec![
            ("byte".into(), CType::Integer { rank: IntRank::Char, unsigned: false }),
            ("wide".into(), CType::Integer { rank: IntRank::Long, unsigned: false }),
        ], Target::default()).unwrap();
        let CType::Union { members, size, align, .. } = &union else { panic!("union expected") };
        assert_eq!((*size, *align), (8,8));
        assert!(members.iter().all(|m| m.offset == 0));
        let structure = layout_struct(None, vec![
            ("first".into(), CType::int()),
            ("part".into(), union),
        ], Target::default()).unwrap();
        assert_eq!(structure.size_bytes(Target::default()), Some(16));
    }
    #[test]
    fn x86_64_structure_layout_and_tail_padding() {
        let ty = layout_struct(Some("Sample".into()), vec![
            ("a".into(), CType::Integer { rank: IntRank::Char, unsigned: false }),
            ("b".into(), CType::int()),
            ("c".into(), CType::Integer { rank: IntRank::Long, unsigned: false }),
        ], Target::default()).unwrap();
        let CType::Struct { members, size, align, .. } = &ty else { panic!("struct expected") };
        assert_eq!((*size, *align), (16,8));
        assert_eq!(members.iter().map(|m| m.offset).collect::<Vec<_>>(), [0,4,8]);
        let nested = layout_struct(None, vec![
            ("first".into(), ty),
            ("tail".into(), CType::Integer { rank: IntRank::Char, unsigned: false }),
        ], Target::default()).unwrap();
        assert_eq!(nested.size_bytes(Target::default()), Some(24));
        assert_eq!(nested.align_bytes(Target::default()), Some(8));
        assert!(layout_struct(None, vec![("a".into(), CType::int()),
            ("a".into(), CType::int())], Target::default()).is_err());
    }
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
    fn compatible_incomplete_array_declarations_merge_bounds() {
        let incomplete = CType::Array(Box::new(CType::int()), None);
        let complete = CType::Array(Box::new(CType::int()), Some(4));
        assert_eq!(incomplete.composite(&complete), Some(complete.clone()));
        assert_eq!(complete.composite(&incomplete), Some(complete.clone()));
        assert_eq!(complete.composite(&complete), Some(complete.clone()));
        assert_eq!(complete.composite(&CType::Array(Box::new(CType::int()), Some(5))), None);
        assert_eq!(incomplete.composite(&CType::Array(Box::new(
            CType::Integer { rank: IntRank::Char, unsigned: false }
        ), Some(4))), None);
    }
    #[test]
    fn invalid_target_is_diagnostic() {
        assert!(Target { int_bits: 8, ..Target::default() }.validate().is_err());
    }
}
