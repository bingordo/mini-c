//! Central source-builtin registry for checked Mini-C expression semantics.
//!
//! Keep recognition, arity and required constant-argument positions independent
//! of individual expression checking branches. A registered builtin is never
//! converted into an ordinary external C call merely because its IR lowering
//! has not been implemented. Frontend adapters may consume the same stable IDs.
/// Exact unsigned operand bit-count operation; zero is undefined for clz/ctz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitCountKind { LeadingZero, TrailingZero, Population }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinId {
    Ctz32,
    BitCount(BitCountKind, crate::IntRank),
    ByteSwap(u8),
    Nan(u8),
    IsNan,
    AtomicLoadN,
    AtomicStoreN,
    AtomicExchangeN,
    AtomicFetchAdd,
    AtomicFetchSub,
    AtomicFetchAnd,
    AtomicFetchOr,
    AtomicFetchXor,
    AtomicCompareExchangeN,
    AtomicThreadFence,
    AtomicSignalFence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoweringClass {
    /// Checked and implemented in the existing expression/backend path.
    IntegerCtz,
    IntegerBitCount,
    IntegerByteSwap,
    FloatNan,
    FloatClassify,
    /// Source contract is recognized, but must not use generic call lowering.
    AtomicIrRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinSpec {
    pub id: BuiltinId,
    pub name: &'static str,
    pub argument_count: usize,
    /// Index of a GNU C11 integer memory-order expression, when present.
    pub memory_order_argument: Option<usize>,
    pub failure_order_argument: Option<usize>,
    pub weak_argument: Option<usize>,
    pub lowering: LoweringClass,
}

const fn atomic(id: BuiltinId, name: &'static str, count: usize, order: usize,
    failure: Option<usize>, weak: Option<usize>) -> BuiltinSpec
{
    BuiltinSpec {
        id, name, argument_count: count, memory_order_argument: Some(order),
        failure_order_argument: failure, weak_argument: weak,
        lowering: LoweringClass::AtomicIrRequired,
    }
}

/// Fixed spellings and signatures. Unsupported by-address GNU atomic overloads
/// are intentionally absent; source diagnostics must not infer their ABI.
pub const REGISTRY: [BuiltinSpec; 26] = [
    BuiltinSpec {
        id: BuiltinId::Ctz32, name: "__builtin_ctz", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None,
        weak_argument: None, lowering: LoweringClass::IntegerCtz,
    },
    BuiltinSpec {
        id: BuiltinId::ByteSwap(16), name: "__builtin_bswap16", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None,
        weak_argument: None, lowering: LoweringClass::IntegerByteSwap,
    },
    BuiltinSpec {
        id: BuiltinId::ByteSwap(32), name: "__builtin_bswap32", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None,
        weak_argument: None, lowering: LoweringClass::IntegerByteSwap,
    },
    BuiltinSpec {
        id: BuiltinId::ByteSwap(64), name: "__builtin_bswap64", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None,
        weak_argument: None, lowering: LoweringClass::IntegerByteSwap,
    },
    BuiltinSpec { id: BuiltinId::BitCount(BitCountKind::LeadingZero, crate::IntRank::Int), name: "__builtin_clz", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None, weak_argument: None, lowering: LoweringClass::IntegerBitCount },
    BuiltinSpec { id: BuiltinId::BitCount(BitCountKind::LeadingZero, crate::IntRank::Long), name: "__builtin_clzl", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None, weak_argument: None, lowering: LoweringClass::IntegerBitCount },
    BuiltinSpec { id: BuiltinId::BitCount(BitCountKind::LeadingZero, crate::IntRank::LongLong), name: "__builtin_clzll", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None, weak_argument: None, lowering: LoweringClass::IntegerBitCount },
    BuiltinSpec { id: BuiltinId::BitCount(BitCountKind::TrailingZero, crate::IntRank::Long), name: "__builtin_ctzl", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None, weak_argument: None, lowering: LoweringClass::IntegerBitCount },
    BuiltinSpec { id: BuiltinId::BitCount(BitCountKind::TrailingZero, crate::IntRank::LongLong), name: "__builtin_ctzll", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None, weak_argument: None, lowering: LoweringClass::IntegerBitCount },
    BuiltinSpec { id: BuiltinId::BitCount(BitCountKind::Population, crate::IntRank::Int), name: "__builtin_popcount", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None, weak_argument: None, lowering: LoweringClass::IntegerBitCount },
    BuiltinSpec { id: BuiltinId::BitCount(BitCountKind::Population, crate::IntRank::Long), name: "__builtin_popcountl", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None, weak_argument: None, lowering: LoweringClass::IntegerBitCount },
    BuiltinSpec { id: BuiltinId::BitCount(BitCountKind::Population, crate::IntRank::LongLong), name: "__builtin_popcountll", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None, weak_argument: None, lowering: LoweringClass::IntegerBitCount },
    BuiltinSpec {
        id: BuiltinId::Nan(32), name: "__builtin_nanf", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None,
        weak_argument: None, lowering: LoweringClass::FloatNan,
    },
    BuiltinSpec {
        id: BuiltinId::Nan(64), name: "__builtin_nan", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None,
        weak_argument: None, lowering: LoweringClass::FloatNan,
    },
    BuiltinSpec {
        id: BuiltinId::IsNan, name: "__builtin_isnan", argument_count: 1,
        memory_order_argument: None, failure_order_argument: None,
        weak_argument: None, lowering: LoweringClass::FloatClassify,
    },
    atomic(BuiltinId::AtomicLoadN, "__atomic_load_n", 2, 1, None, None),
    atomic(BuiltinId::AtomicStoreN, "__atomic_store_n", 3, 2, None, None),
    atomic(BuiltinId::AtomicExchangeN, "__atomic_exchange_n", 3, 2, None, None),
    atomic(BuiltinId::AtomicFetchAdd, "__atomic_fetch_add", 3, 2, None, None),
    atomic(BuiltinId::AtomicFetchSub, "__atomic_fetch_sub", 3, 2, None, None),
    atomic(BuiltinId::AtomicFetchAnd, "__atomic_fetch_and", 3, 2, None, None),
    atomic(BuiltinId::AtomicFetchOr, "__atomic_fetch_or", 3, 2, None, None),
    atomic(BuiltinId::AtomicFetchXor, "__atomic_fetch_xor", 3, 2, None, None),
    atomic(BuiltinId::AtomicCompareExchangeN, "__atomic_compare_exchange_n",
        6, 4, Some(5), Some(3)),
    atomic(BuiltinId::AtomicThreadFence, "__atomic_thread_fence", 1, 0, None, None),
    atomic(BuiltinId::AtomicSignalFence, "__atomic_signal_fence", 1, 0, None, None),
];

/// Independent source C11 order oracle used before backend IR construction.
/// Validity deliberately does not depend on compiler-ir, preserving frontend
/// layering; compiler-ci tests the two registries against one another.
pub fn valid_atomic_orders(id: BuiltinId, success: u8, failure: Option<u8>) -> bool {
    if success > 5 { return false; }
    match id {
        BuiltinId::Ctz32 | BuiltinId::ByteSwap(_) | BuiltinId::BitCount(..) | BuiltinId::Nan(_) | BuiltinId::IsNan => false,
        BuiltinId::AtomicLoadN => failure.is_none() && matches!(success, 0|1|2|5),
        BuiltinId::AtomicStoreN => failure.is_none() && matches!(success, 0|3|5),
        BuiltinId::AtomicCompareExchangeN => {
            let Some(failure) = failure else { return false };
            match success {
                0 => failure == 0,
                1 => matches!(failure, 0|1),
                2 => matches!(failure, 0..=2),
                3 => failure == 0,
                4 => matches!(failure, 0..=2),
                5 => matches!(failure, 0|1|2|5),
                _ => false,
            }
        }
        _ => failure.is_none(),
    }
}

pub fn lookup(name: &str) -> Option<&'static BuiltinSpec> {
    REGISTRY.iter().find(|entry| entry.name == name)
}

/// Unknown compiler-reserved builtin spellings must be diagnosed rather than
/// routed to the ordinary unresolved/external function call path.
pub fn reserved_name(name: &str) -> bool {
    name.starts_with("__builtin_") || name.starts_with("__atomic_")
        || name.starts_with("__sync_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn memory_order_matrix_rejects_out_of_range_and_wrong_failure_shapes() {
        // The source checker must reject invalid order encodings before any
        // target-specific legalization or native code generation is attempted.
        for spec in REGISTRY {
            if spec.lowering != LoweringClass::AtomicIrRequired {
                continue;
            }
            for success in 0..=u8::MAX {
                if success > 5 {
                    assert!(!valid_atomic_orders(spec.id, success, None),
                        "{} accepted invalid order {success}", spec.name);
                    assert!(!valid_atomic_orders(spec.id, success, Some(0)),
                        "{} accepted invalid success order {success}", spec.name);
                }
            }
            if spec.id == BuiltinId::AtomicCompareExchangeN {
                for success in 0..=5 {
                    assert!(!valid_atomic_orders(spec.id, success, None),
                        "CAS accepted a missing failure order");
                    for failure in 6..=u8::MAX {
                        assert!(!valid_atomic_orders(spec.id, success, Some(failure)),
                            "CAS accepted invalid failure order {failure}");
                    }
                }
            } else {
                for success in 0..=5 {
                    assert!(!valid_atomic_orders(spec.id, success, Some(0)),
                        "{} accepted unexpected failure order", spec.name);
                }
            }
        }
    }

    #[test]
    fn registry_is_unique_and_has_bounded_order_operand_indices() {
        let mut names = HashSet::new();
        for spec in REGISTRY {
            assert!(names.insert(spec.name), "duplicate builtin {}", spec.name);
            assert_eq!(lookup(spec.name), Some(&spec));
            assert!(spec.argument_count > 0);
            for index in [spec.memory_order_argument, spec.failure_order_argument,
                spec.weak_argument].into_iter().flatten()
            {
                assert!(index < spec.argument_count);
            }
            if matches!(spec.lowering, LoweringClass::IntegerCtz) {
                assert_eq!(spec.id, BuiltinId::Ctz32);
                assert_eq!(spec.argument_count, 1);
                assert_eq!(spec.memory_order_argument, None);
            } else if let BuiltinId::BitCount(..)=spec.id {
                assert_eq!(spec.argument_count,1);
                assert_eq!(spec.memory_order_argument,None);
            } else if let BuiltinId::Nan(bits)=spec.id {
                assert!(matches!(bits,32|64));
                assert_eq!(spec.argument_count,1);
                assert_eq!(spec.lowering,LoweringClass::FloatNan);
            } else if let BuiltinId::IsNan=spec.id {
                assert_eq!(spec.argument_count,1);
                assert_eq!(spec.lowering,LoweringClass::FloatClassify);
            } else if let BuiltinId::ByteSwap(bits)=spec.id {
                assert!(matches!(bits,16|32|64));
                assert_eq!(spec.argument_count,1);
                assert_eq!(spec.memory_order_argument,None);
            } else {
                assert!(spec.name.starts_with("__atomic_"));
                assert!(spec.memory_order_argument.is_some());
            }
        }
        for name in ["__atomic_load", "__sync_fetch_and_add", "__builtin_fake"] {
            assert!(reserved_name(name));
            assert!(lookup(name).is_none());
        }
        assert!(!reserved_name("user_function"));
    }
}
