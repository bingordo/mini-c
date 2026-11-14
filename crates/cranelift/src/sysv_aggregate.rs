//! Initial x86-64 SysV aggregate eightbyte classifier for the Cranelift adapter.
//!
//! Supports integer/pointer and float/double eightbyte classes. Extended x87,
//! register exhaustion and actual call/return lowering remain separate work.
//! Do not use a partial classification to emit a call: an error is required.
use mini_c_sema::{CType, Target};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EightbyteClass { Integer, Sse }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AggregateAbi {
    Registers(Vec<EightbyteClass>),
    Memory,
}

pub fn classify_aggregate(ty: &CType) -> Result<AggregateAbi, String> {
    let size = ty.size_bytes(Target::default())
        .ok_or("incomplete aggregate in SysV ABI")?;
    if !matches!(ty.unqualified(), CType::Struct { .. } | CType::Union { .. }) {
        return Err("expected a struct or union".into());
    }
    if size > 16 { return Ok(AggregateAbi::Memory); }
    if size == 0 { return Err("zero-sized aggregate is unsupported".into()); }
    let mut slots = vec![None; size.div_ceil(8) as usize];
    if visit(ty, 0, &mut slots)? { return Ok(AggregateAbi::Memory); }
    Ok(AggregateAbi::Registers(slots.into_iter()
        .map(|slot| slot.unwrap_or(EightbyteClass::Integer)).collect()))
}

// Returns true if any field forces the complete aggregate to MEMORY.
fn visit(ty: &CType, offset: u64, slots: &mut [Option<EightbyteClass>]) -> Result<bool, String> {
    match ty.unqualified() {
        CType::Struct { members, .. } | CType::Union { members, .. } => {
            for member in members {
                if visit(&member.ty, offset.checked_add(member.offset)
                    .ok_or("aggregate offset overflow")?, slots)? { return Ok(true); }
            }
        }
        CType::Array(element, Some(count)) => {
            let stride = element.size_bytes(Target::default())
                .ok_or("incomplete array element in aggregate")?;
            for i in 0..*count {
                if visit(element, offset.checked_add(i.checked_mul(stride)
                    .ok_or("aggregate offset overflow")?)
                    .ok_or("aggregate offset overflow")?, slots)? { return Ok(true); }
            }
        }
        CType::Integer { .. } | CType::Pointer(_) |
        CType::Floating(mini_c_syntax::ast::FloatRank::Float | mini_c_syntax::ast::FloatRank::Double) => {
            let size = ty.size_bytes(Target::default()).ok_or("invalid scalar size")?;
            let align = ty.align_bytes(Target::default()).ok_or("invalid scalar alignment")?;
            if !offset.is_multiple_of(align) {
                return Ok(true);
            }
            if size == 0 { return Err("zero-sized field".into()); }
            let end = offset.checked_add(size - 1).ok_or("field offset overflow")?;
            let first = usize::try_from(offset / 8).map_err(|_| "field index overflow")?;
            let last = usize::try_from(end / 8).map_err(|_| "field index overflow")?;
            for index in first..=last {
                let slot = slots.get_mut(index).ok_or("field outside aggregate bounds")?;
                let incoming = if matches!(ty.unqualified(), CType::Floating(_)) {
                    EightbyteClass::Sse
                } else { EightbyteClass::Integer };
                *slot = Some(if *slot == Some(EightbyteClass::Integer) || incoming == EightbyteClass::Integer {
                    EightbyteClass::Integer
                } else { EightbyteClass::Sse });
            }
        }
        CType::Floating(_) => return Err("x87/extended FP aggregate ABI classification not implemented".into()),
        _ => return Err("unsupported aggregate member ABI type".into()),
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mini_c_sema::types::{layout_struct, layout_union};

    #[test]
    fn integer_register_classes() {
        let t = Target::default();
        let one = layout_struct(None, vec![("x".into(), CType::int())], t).unwrap();
        assert_eq!(classify_aggregate(&one).unwrap(),
            AggregateAbi::Registers(vec![EightbyteClass::Integer]));
        let two = layout_struct(None, vec![
            ("a".into(), CType::Pointer(Box::new(CType::int()))),
            ("b".into(), CType::Pointer(Box::new(CType::int()))),
        ], t).unwrap();
        assert_eq!(classify_aggregate(&two).unwrap(),
            AggregateAbi::Registers(vec![EightbyteClass::Integer, EightbyteClass::Integer]));
        let union = layout_union(None, vec![
            ("a".into(), CType::int()),
            ("b".into(), CType::Pointer(Box::new(CType::int()))),
        ], t).unwrap();
        assert_eq!(classify_aggregate(&union).unwrap(),
            AggregateAbi::Registers(vec![EightbyteClass::Integer]));
    }

    #[test]
    fn mixed_integer_and_sse_classes() {
        use mini_c_syntax::ast::FloatRank;
        let t = Target::default();
        let mixed = layout_struct(None, vec![
            ("x".into(), CType::Floating(FloatRank::Double)),
            ("y".into(), CType::int()),
        ], t).unwrap();
        assert_eq!(classify_aggregate(&mixed).unwrap(),
            AggregateAbi::Registers(vec![EightbyteClass::Sse, EightbyteClass::Integer]));
        let both = layout_struct(None, vec![
            ("x".into(), CType::Floating(FloatRank::Float)),
            ("y".into(), CType::Floating(FloatRank::Float)),
        ], t).unwrap();
        assert_eq!(classify_aggregate(&both).unwrap(),
            AggregateAbi::Registers(vec![EightbyteClass::Sse]));
        let union = layout_union(None, vec![
            ("x".into(), CType::Floating(FloatRank::Double)),
            ("y".into(), CType::Pointer(Box::new(CType::int()))),
        ], t).unwrap();
        assert_eq!(classify_aggregate(&union).unwrap(),
            AggregateAbi::Registers(vec![EightbyteClass::Integer]));
    }

    #[test]
    fn large_aggregate_uses_memory() {
        let t = Target::default();
        let large = layout_struct(None, vec![
            ("a".into(), CType::Pointer(Box::new(CType::int()))),
            ("b".into(), CType::Pointer(Box::new(CType::int()))),
            ("c".into(), CType::Pointer(Box::new(CType::int()))),
        ], t).unwrap();
        assert_eq!(classify_aggregate(&large).unwrap(), AggregateAbi::Memory);
    }
}
