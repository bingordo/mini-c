//! Target-dependent C integer types and the conversions used by the supported subset.
//! No host sizeof() assumptions: a caller selects the integer model.
use mini_c_syntax::ast::{FloatRank, Span, TagKind};
use mini_c_syntax::diag::Diagnostic;
use mini_c_syntax::integer_literal::IntegerLiteral;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntRank { Bool, Char, Short, Int, Long, LongLong }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitField {
    /// Bit position within the field's declared 8/16/32/64-bit little-endian unit.
    pub shift: u8,
    pub width: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructMember {
    pub name: String,
    pub ty: CType,
    pub offset: u64,
    /// C99 bit-field storage information. The declared scalar type stays
    /// ordinary int/unsigned int; bit-fields cannot be addressed.
    pub bit_field: Option<BitField>,
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
    /// Retain top-level const independently from runtime representation.
    Qualified(Box<CType>),
    /// A volatile-qualified C object. Never discard before native capability checking.
    Volatile(Box<CType>),
    /// A C11 atomic-qualified integer object. Its layout remains exact-width,
    /// but its memory accesses are NOT ordinary C loads/stores. Every backend
    /// must explicitly lower them or reject them before object emission.
    Atomic(Box<CType>),
    Integer { rank: IntRank, unsigned: bool },
    /// Checked hosted FP type identity is retained in declarations. Native FP
    /// ABI/code generation is not yet implemented; never alias these to int.
    Floating(FloatRank),
    Pointer(Box<CType>),
    /// Incomplete arrays use None; bound and full object layout are target-dependent.
    Array(Box<CType>, Option<u64>),
    Function { result: Box<CType>, params: Vec<CType> },
    UnprototypedFunction { result: Box<CType> },
    VariadicFunction { result: Box<CType>, params: Vec<CType> },
}

impl CType {
    /// Remove top-level const only. Nested pointee const must be retained.
    pub fn unqualified(&self) -> &Self {
        match self {
            Self::Qualified(inner) | Self::Volatile(inner) | Self::Atomic(inner) =>
                inner.unqualified(),
            other => other,
        }
    }
    pub fn is_const(&self) -> bool {
        match self {
            Self::Qualified(_) => true,
            Self::Volatile(inner) | Self::Atomic(inner) => inner.is_const(),
            _ => false,
        }
    }
    pub fn is_volatile(&self) -> bool {
        match self {
            Self::Volatile(_) => true,
            Self::Qualified(inner) | Self::Atomic(inner) => inner.is_volatile(),
            _ => false,
        }
    }
    pub fn contains_volatile(&self) -> bool {
        match self {
            Self::Volatile(_) => true,
            Self::Qualified(inner) | Self::Atomic(inner) |
            Self::Pointer(inner) | Self::Array(inner, _) =>
                inner.contains_volatile(),
            Self::UnprototypedFunction { result } => result.contains_volatile(),
            Self::Function { result, params } |
            Self::VariadicFunction { result, params } =>
                result.contains_volatile() || params.iter().any(Self::contains_volatile),
            Self::Struct { members, .. } | Self::Union { members, .. } =>
                members.iter().any(|field| field.ty.contains_volatile()),
            _ => false,
        }
    }
    /// True if the immediate object is C11 atomic-qualified.
    pub fn is_atomic(&self) -> bool {
        match self {
            Self::Atomic(_) => true,
            Self::Qualified(inner) | Self::Volatile(inner) => inner.is_atomic(),
            _ => false,
        }
    }
    /// Scan C object/member/function composition for atomic storage. This
    /// catches pointer pointees and lvalues whose rvalue lost top qualifiers.
    pub fn contains_atomic(&self) -> bool {
        match self {
            Self::Atomic(_) => true,
            Self::Qualified(inner) | Self::Volatile(inner) |
            Self::Pointer(inner) | Self::Array(inner, _) => inner.contains_atomic(),
            Self::UnprototypedFunction { result } => result.contains_atomic(),
            Self::Function {result,params} | Self::VariadicFunction {result,params} =>
                result.contains_atomic() || params.iter().any(Self::contains_atomic),
            Self::Struct {members,..} | Self::Union {members,..} =>
                members.iter().any(|field|field.ty.contains_atomic()),
            _ => false,
        }
    }
    /// Preserve one explicit atomic qualifier on a validated integer object.
    pub fn with_atomic(self) -> Self {
        Self::Atomic(Box::new(self))
    }
    pub fn with_const(self) -> Self {
        match self {
            Self::Array(element, count) =>
                Self::Array(Box::new((*element).with_const()), count),
            Self::Qualified(_) => self,
            other => Self::Qualified(Box::new(other)),
        }
    }
    pub fn with_volatile(self) -> Self {
        match self {
            Self::Array(element, count) =>
                Self::Array(Box::new((*element).with_volatile()), count),
            Self::Qualified(inner) =>
                Self::Qualified(Box::new((*inner).with_volatile())),
            Self::Volatile(_) => self,
            other => Self::Volatile(Box::new(other)),
        }
    }
    /// C pointer assignment may add, but not discard, immediate pointee const.
    /// Nested pointer targets must be exactly compatible (not T** -> const T**).
    pub fn compatible_pointee_assignment(dest: &Self, source: &Self) -> bool {
        if source.is_const() && !dest.is_const() { return false; }
        if source.is_volatile() && !dest.is_volatile() { return false; }
        // Atomic and ordinary pointees do not have interchangeable memory
        // semantics. Require exact qualification until explicit conversions
        // and their atomic-aware operations are separately validated.
        if source.is_atomic()!=dest.is_atomic() { return false; }
        let (dest, source) = (dest.unqualified(), source.unqualified());
        dest.composite(source).is_some()
            || (matches!(dest, Self::Void) && source.is_object())
            || (matches!(source, Self::Void) && dest.is_object())
    }
    /// Target-specific object alignment in bytes, including wasm32 ILP32.
    pub fn align_bytes(&self, target: Target) -> Option<u64> {
        match self {
            Self::Void | Self::Function { .. } | Self::VariadicFunction { .. } | Self::UnprototypedFunction { .. } | Self::TagRef { .. } => None,
            Self::Qualified(inner) | Self::Volatile(inner) | Self::Atomic(inner) =>
                inner.align_bytes(target),
            Self::Struct { align, .. } | Self::Union { align, .. } => Some(*align),
            Self::Array(element, _) => element.align_bytes(target),
            Self::Pointer(_) => Some(u64::from(target.pointer_bits / 8)),
            Self::Integer { rank, .. } => Some(u64::from(target.bits(*rank) / 8)),
            Self::Floating(rank) => Some(match rank {
                FloatRank::Float => 4, FloatRank::Double => 8,
                FloatRank::LongDouble => 16, // x86-64 SysV padded x87 format
                FloatRank::Float128 => 16,  // IEEE binary128, distinct from x87
            }),
        }
    }

    /// A named aggregate and its recursive/incomplete reference share
    /// identity even though one representation does not embed the members.
    pub fn same_type(&self, other: &Self) -> bool {
        use CType::{Struct, Union, TagRef};
        match (self, other) {
            (CType::Qualified(a), CType::Qualified(b)) |
            (CType::Volatile(a), CType::Volatile(b)) |
            (CType::Atomic(a), CType::Atomic(b)) => a.same_type(b),
            (CType::Qualified(_), _) | (_, CType::Qualified(_)) |
            (CType::Volatile(_), _) | (_, CType::Volatile(_)) |
            (CType::Atomic(_), _) | (_, CType::Atomic(_)) => false,
            (Struct { identity: a, .. }, Struct { identity: b, .. }) |
            (Union { identity: a, .. }, Union { identity: b, .. }) =>
                a == b,
            (Struct { identity: a, .. }, TagRef { kind: TagKind::Struct, identity: b, .. }) |
            (TagRef { kind: TagKind::Struct, identity: a, .. }, Struct { identity: b, .. }) |
            (Union { identity: a, .. }, TagRef { kind: TagKind::Union, identity: b, .. }) |
            (TagRef { kind: TagKind::Union, identity: a, .. }, Union { identity: b, .. }) =>
                a == b,
            (CType::Pointer(a), CType::Pointer(b)) => a.same_type(b),
            (CType::UnprototypedFunction { .. }, _) | (_, CType::UnprototypedFunction { .. }) =>
                self.composite(other).is_some(),
            // C17 6.7.6.3p15 ignores a function parameter's top-level
            // qualifiers when determining compatibility, including through
            // pointers to functions.
            (CType::Function { result: ar, params: ap },
             CType::Function { result: br, params: bp }) |
            (CType::VariadicFunction { result: ar, params: ap },
             CType::VariadicFunction { result: br, params: bp }) =>
                ar.same_type(br) && ap.len() == bp.len()
                && ap.iter().zip(bp).all(|(a, b)|
                    a.unqualified().same_type(b.unqualified())),
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
            (Self::Qualified(a), Self::Qualified(b)) =>
                Some(Self::Qualified(Box::new(a.composite(b)?))),
            (Self::Volatile(a), Self::Volatile(b)) =>
                Some(Self::Volatile(Box::new(a.composite(b)?))),
            (Self::Atomic(a), Self::Atomic(b)) =>
                Some(Self::Atomic(Box::new(a.composite(b)?))),
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
            (Self::UnprototypedFunction { result: a }, Self::UnprototypedFunction { result: b }) =>
                Some(Self::UnprototypedFunction { result: Box::new(a.composite(b)?) }),
            (Self::UnprototypedFunction { result: a }, Self::Function { result: b, params })
            | (Self::Function { result: b, params }, Self::UnprototypedFunction { result: a }) => {
                // A prototype is compatible only if every parameter is unchanged
                // by the default argument promotions (C99 6.7.5.3p15).
                if params.iter().any(|p| matches!(p.unqualified(),
                    Self::Integer { rank: IntRank::Bool | IntRank::Char | IntRank::Short, .. }
                    | Self::Floating(FloatRank::Float))) {
                    return None;
                }
                Some(Self::Function { result: Box::new(a.composite(b)?),
                    params: params.clone() })
            }
            (Self::UnprototypedFunction { .. }, _) | (_, Self::UnprototypedFunction { .. }) => None,
            (Self::VariadicFunction { result: ar, params: ap },
             Self::VariadicFunction { result: br, params: bp }) if ap.len() == bp.len() => {
                let result = Box::new(ar.composite(br)?);
                let params = ap.iter().zip(bp)
                    .map(|(a,b)| a.unqualified().composite(b.unqualified()))
                    .collect::<Option<Vec<_>>>()?;
                Some(Self::VariadicFunction { result, params })
            }
            (Self::Function { result: ar, params: ap },
             Self::Function { result: br, params: bp }) if ap.len() == bp.len() => {
                let result = Box::new(ar.composite(br)?);
                // Top-level qualifiers on C function parameter types are ignored
                // when determining compatibility (C17 6.7.6.3p15). Keep
                // qualifiers nested under pointers: const int * differs
                // from int *, but int *const does not differ from int *.
                let params = ap.iter().zip(bp)
                    .map(|(a,b)| a.unqualified().composite(b.unqualified()))
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

    pub fn is_integer(&self) -> bool {
        matches!(self.unqualified(), Self::Integer { .. })
    }
    pub fn is_scalar(&self) -> bool {
        matches!(self.unqualified(), Self::Integer { .. } | Self::Floating(_) | Self::Pointer(_))
    }
    /// C99 6.7.2.1: a struct with a flexible array member (or a union
    /// containing one recursively) cannot be an array element or struct member.
    pub fn has_flexible_array_member(&self) -> bool {
        match self.unqualified() {
            Self::Struct { members, .. } => members.last().is_some_and(|member|
                matches!(member.ty.unqualified(), Self::Array(_, None))),
            Self::Union { members, .. } => members.iter().any(|member|
                member.ty.has_flexible_array_member()),
            Self::Array(element, _) => element.has_flexible_array_member(),
            _ => false,
        }
    }

    pub fn is_object(&self) -> bool {
        !matches!(self.unqualified(), Self::Void | Self::Function { .. } | Self::VariadicFunction { .. } | Self::UnprototypedFunction { .. })
    }

    /// Object size in bytes for the selected C target data model.
    /// Incomplete arrays, void/function types and overflowing extents have no size.
    pub fn size_bytes(&self, target: Target) -> Option<u64> {
        match self {
            Self::Void | Self::Function { .. } | Self::VariadicFunction { .. } | Self::UnprototypedFunction { .. } | Self::TagRef { .. } => None,
            Self::Qualified(inner) | Self::Volatile(inner) | Self::Atomic(inner) =>
                inner.size_bytes(target),
            Self::Struct { size, .. } | Self::Union { size, .. } => Some(*size),
            Self::Integer { rank, .. } => Some(u64::from(target.bits(*rank) / 8)),
            Self::Floating(rank) => Some(match rank {
                FloatRank::Float => 4, FloatRank::Double => 8,
                FloatRank::LongDouble => 16, // x86-64 SysV padded x87 format
                FloatRank::Float128 => 16,  // distinct IEEE binary128 storage
            }),
            Self::Pointer(_) => Some(u64::from(target.pointer_bits / 8)),
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
/// Named/unnamed integer and _Bool bit-fields follow the x86-64 SysV/GCC layout.
pub fn layout_struct(
    tag: Option<String>, fields: Vec<(String, CType)>, target: Target,
) -> Result<CType, &'static str> {
    layout_struct_with_bits(tag,
        fields.into_iter().map(|(name, ty)| (name, ty, None)).collect(), target)
}

pub fn layout_struct_with_bits(
    tag: Option<String>, fields: Vec<(String, CType, Option<u8>)>, target: Target,
) -> Result<CType, &'static str> {
    if fields.is_empty() { return Err("empty structures are unsupported"); }
    let mut size = 0u64;
    let mut align = 1u64;
    let mut bit_cursor: Option<u64> = None;
    let mut members = Vec::with_capacity(fields.len());
    let mut seen = std::collections::HashSet::new();
    let field_count = fields.len();
    for (index, (name, ty, bit_width)) in fields.into_iter().enumerate() {
        let unnamed = name.is_empty();
        if unnamed && bit_width.is_none() {
            return Err("unnamed aggregate member requires a bit-field width");
        }
        if !unnamed && !seen.insert(name.clone()) {
            return Err("duplicate structure member");
        }
        if ty.has_flexible_array_member() {
            return Err("structure member cannot contain a flexible array member");
        }
        if let CType::Array(element, None) = ty.unqualified() {
            if index + 1 != field_count || members.is_empty() || bit_width.is_some()
                || element.size_bytes(target).filter(|size| *size > 0).is_none()
                || !element.is_object()
            {
                return Err("invalid flexible array member");
            }
            let field_align = element.align_bytes(target).ok_or("incomplete flexible array element")?;
            align = align.max(field_align);
            let offset = align_up(size, field_align).ok_or("structure member layout overflow")?;
            size = offset;
            members.push(StructMember { name, ty, offset, bit_field: None });
            continue;
        }
        let field_size = ty.size_bytes(target).ok_or("structure member has incomplete type")?;
        let field_align = ty.align_bytes(target).ok_or("structure member has incomplete type")?;
        if field_size == 0 || !ty.is_object() {
            return Err("structure member must have a complete object type");
        }
        if let Some(width) = bit_width {
            let unit_bits = match ty.unqualified() {
                CType::Integer { rank: IntRank::Bool | IntRank::Char
                    | IntRank::Short | IntRank::Int | IntRank::Long, .. } =>
                    u64::from(target.bits(match ty.unqualified() {
                        CType::Integer { rank, .. } => *rank,
                        _ => unreachable!(),
                    })),
                _ => return Err("unsupported bit-field type or width"),
            };
            if !matches!(unit_bits, 8 | 16 | 32 | 64) || u64::from(width) > unit_bits {
                return Err("unsupported bit-field type or width");
            }
            if width == 0 && !unnamed {
                return Err("unsupported bit-field type or width");
            }
            // An unnamed zero-width field advances to its *declared* type's
            // storage-unit boundary (8/16/32/64 bits). It does not
            // impose alignment on its enclosing struct.
            if width == 0 {
                let position = align_up(bit_cursor.unwrap_or(size * 8), unit_bits)
                    .ok_or("bit-field position overflow")?;
                size = size.max(position / 8);
                bit_cursor = None;
                continue;
            }
            // Only named fields impose their declared alignment; unnamed
            // fields still consume bits but have no initializer slots.
            if !unnamed { align = align.max(field_align); }
            // Mixed base-type groups share a global bit cursor. Each field
            // must fit wholly within an allocation unit of its own type.
            let mut position = bit_cursor.unwrap_or_else(|| size * 8);
            let end = position.checked_add(u64::from(width))
                .ok_or("bit-field position overflow")?;
            if position / unit_bits != (end - 1) / unit_bits {
                position = align_up(position, unit_bits)
                    .ok_or("bit-field position overflow")?;
            }
            let end = position.checked_add(u64::from(width))
                .ok_or("bit-field position overflow")?;
            let offset = position / unit_bits * (unit_bits / 8);
            size = size.max(end.checked_add(7)
                .ok_or("bit-field position overflow")? / 8);
            bit_cursor = Some(end);
            if !unnamed {
                members.push(StructMember {
                    name, ty, offset,
                    bit_field: Some(BitField {
                        shift: u8::try_from(position % unit_bits).map_err(|_| "bit shift overflow")?,
                        width,
                    }),
                });
            }
        } else {
            bit_cursor = None;
            align = align.max(field_align);
            let offset = align_up(size, field_align)
                .ok_or("structure member layout overflow")?;
            size = offset.checked_add(field_size).ok_or("structure size overflow")?;
            members.push(StructMember { name, ty, offset, bit_field: None });
        }
    }
    if members.is_empty() { return Err("empty structures are unsupported"); }
    size = align_up(size, align).ok_or("structure size overflow")?;
    Ok(CType::Struct { identity: 0, tag, members, size, align })
}

/// C union members all start at byte zero and share the maximum size
/// rounded to the strictest member alignment.
pub fn layout_union(
    tag: Option<String>, fields: Vec<(String, CType)>, target: Target,
) -> Result<CType, &'static str> {
    layout_union_with_bits(tag,
        fields.into_iter().map(|(name, ty)| (name, ty, None)).collect(), target)
}

pub fn layout_union_with_bits(
    tag: Option<String>, fields: Vec<(String, CType, Option<u8>)>, target: Target,
) -> Result<CType, &'static str> {
    if fields.is_empty() { return Err("empty unions are unsupported"); }
    let mut size = 0u64;
    let mut align = 1u64;
    let mut members = Vec::with_capacity(fields.len());
    let mut seen = std::collections::HashSet::new();
    for (name, ty, bit_width) in fields {
        let unnamed = name.is_empty();
        if unnamed && bit_width.is_none() {
            return Err("unnamed aggregate member requires a bit-field width");
        }
        if !unnamed && !seen.insert(name.clone()) {
            return Err("duplicate union member");
        }
        let field_size = ty.size_bytes(target).ok_or("union member has incomplete type")?;
        let field_align = ty.align_bytes(target).ok_or("union member has incomplete type")?;
        if field_size == 0 || !ty.is_object() {
            return Err("union member must have a complete object type");
        }
        if let Some(width) = bit_width {
            let supported = match ty.unqualified() {
                CType::Integer { rank: IntRank::Bool | IntRank::Char
                    | IntRank::Short | IntRank::Int | IntRank::Long, .. } =>
                    u16::from(width) <= match ty.unqualified() {
                        CType::Integer { rank, .. } => target.bits(*rank),
                        _ => unreachable!(),
                    },
                _ => false,
            };
            if !supported || (width == 0 && !unnamed) {
                return Err("unsupported bit-field type or width");
            }
        }
        // Unnamed bit-fields carry neither size/alignment nor an initializer
        // slot in a union, even when their declared type is unsigned int.
        if unnamed { continue; }
        align = align.max(field_align);
        size = size.max(field_size);
        members.push(StructMember {
            name, ty, offset: 0,
            bit_field: bit_width.map(|width| BitField { shift: 0, width }),
        });
    }
    if members.is_empty() { return Err("empty unions are unsupported"); }
    size = align_up(size, align).ok_or("union size overflow")?;
    Ok(CType::Union { identity: 0, tag, members, size, align })
}

#[derive(Debug, Clone, Copy)]
pub struct Target {
    pub char_is_signed: bool,
    pub int_bits: u16,
    pub long_bits: u16,
    /// Object pointer width in bits, independent of C long width.
    pub pointer_bits: u16,
}

impl Default for Target {
    fn default() -> Self {
        Self { char_is_signed: true, int_bits: 32, long_bits: 64, pointer_bits: 64 }
    }
}

impl Target {
    /// WebAssembly32 C data model: 32-bit int, long and object pointers.
    ///
    /// Selecting a checker target does not imply every operation is lowered
    /// by an experimental code generator.
    pub const fn wasm32() -> Self {
        Self { char_is_signed: true, int_bits: 32, long_bits: 32, pointer_bits: 32 }
    }

    pub fn validate(self) -> Result<Self, Diagnostic> {
        if !(16..=64).contains(&self.int_bits) || !(self.int_bits..=64).contains(&self.long_bits)
            || !matches!(self.pointer_bits,32 | 64)
        {
            return Err(Diagnostic::semantic(Span::default(),
                "invalid target: expected 16 <= int_bits <= long_bits <= 64 and 32/64 pointer_bits"));
        }
        Ok(self)
    }

    pub fn bits(self, rank: IntRank) -> u16 {
        match rank { IntRank::Bool | IntRank::Char => 8, IntRank::Short => 16, IntRank::Int => self.int_bits, IntRank::Long => self.long_bits, IntRank::LongLong => 64 }
    }

    pub fn promote(self, ty: &CType) -> CType {
        match ty.unqualified() {
            CType::Integer { rank: IntRank::Bool | IntRank::Char, .. } => CType::int(),
            CType::Integer { rank: IntRank::Short, unsigned } => {
                // C integer promotions: int can represent every short value
                // on normal ILP32/LP64, but unsigned short requires unsigned
                // int on a 16-bit-int target.
                CType::Integer { rank: IntRank::Int,
                    unsigned: *unsigned && self.int_bits <= 16 }
            },
            other => other.clone(),
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

    /// Usual arithmetic conversions for real integer/floating operands.
    /// Preserve the floating result rank in checked IR; native code generation
    /// must independently reject unsupported FP operations and ABI classes.
    pub fn common_arithmetic(self, lhs: &CType, rhs: &CType) -> Option<CType> {
        let lhs = lhs.unqualified();
        let rhs = rhs.unqualified();
        if !(lhs.is_integer() || matches!(lhs, CType::Floating(_)))
            || !(rhs.is_integer() || matches!(rhs, CType::Floating(_)))
        {
            return None;
        }
        use FloatRank::{Float, Double, LongDouble, Float128};
        let floating = [Float128, LongDouble, Double, Float]
            .into_iter().find(|rank|
                matches!(lhs, CType::Floating(r) if r == rank)
                || matches!(rhs, CType::Floating(r) if r == rank));
        if let Some(rank) = floating {
            Some(CType::Floating(rank))
        } else {
            self.common_integer(lhs, rhs)
        }
    }

    /// C17 6.4.4.1 first-representable-type rule.
    /// LongLong has a *distinct rank* and a 64-bit representation even on
    /// models with a narrower long (ILP32). This subset does not model
    /// implementations where long long exceeds 64 bits.
    pub fn integer_literal(self, literal: IntegerLiteral) -> Option<CType> {
        use IntRank::{Int, Long, LongLong as LL};
        let choices: &[(IntRank, bool)] = match (
            literal.nondecimal, literal.unsigned, literal.long, literal.long_long
        ) {
            (_, true, _, true) => &[(LL, true)],
            (false, false, _, true) => &[(LL, false)],
            (true, false, _, true) => &[(LL, false), (LL, true)],
            (_, true, true, false) => &[(Long, true), (LL, true)],
            (false, false, true, false) => &[(Long, false), (LL, false)],
            (true, false, true, false) => &[(Long, false), (Long, true), (LL, false), (LL, true)],
            (_, true, false, false) => &[(Int, true), (Long, true), (LL, true)],
            (false, false, false, false) => &[(Int, false), (Long, false), (LL, false)],
            (true, false, false, false) => &[
                (Int, false), (Int, true), (Long, false), (Long, true),
                (LL, false), (LL, true),
            ],
        };
        for &(rank, unsigned) in choices {
            let bits = self.bits(rank);
            let max = if unsigned {
                (1u128 << bits) - 1
            } else {
                (1u128 << (bits - 1)) - 1
            };
            if u128::from(literal.value) <= max {
                return Some(CType::Integer { rank, unsigned });
            }
        }
        None
    }

    pub fn decimal_literal(self, value: u64) -> Option<CType> {
        for rank in [IntRank::Int, IntRank::Long, IntRank::LongLong] {
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
    fn m10_short_16bit_layout_and_integer_promotions() {
        let lp64 = Target::default();
        let signed = CType::Integer {rank:IntRank::Short,unsigned:false};
        let unsigned = CType::Integer {rank:IntRank::Short,unsigned:true};
        assert_eq!(lp64.bits(IntRank::Short),16);
        assert_eq!(signed.size_bytes(lp64),Some(2));
        assert_eq!(unsigned.align_bytes(lp64),Some(2));
        assert_eq!(lp64.promote(&signed),CType::int());
        assert_eq!(lp64.promote(&unsigned),CType::int());
        assert_eq!(lp64.common_integer(&signed,&unsigned),Some(CType::int()));
        let int16 = Target{int_bits:16,long_bits:32,..lp64};
        assert_eq!(int16.promote(&signed),CType::int());
        assert_eq!(int16.promote(&unsigned),
            CType::Integer{rank:IntRank::Int,unsigned:true});
    }

    #[test]
    fn wasm32_struct_and_union_layout_uses_32bit_pointers_and_long() {
        let target=Target::wasm32().validate().unwrap();
        let members=vec![
            ("c".into(),CType::Integer{rank:IntRank::Char,unsigned:false}),
            ("p".into(),CType::Pointer(Box::new(CType::int()))),
            ("n".into(),CType::Integer{rank:IntRank::Long,unsigned:false}),
        ];
        let structure=layout_struct(Some("S".into()),members.clone(),target).unwrap();
        let CType::Struct{members,size,align,..}=&structure else {panic!("struct")};
        assert_eq!((*size,*align),(12,4));
        assert_eq!(members.iter().map(|m|m.offset).collect::<Vec<_>>(),[0,4,8]);
        let union=layout_union(Some("U".into()),members.iter().map(|m|(m.name.clone(),m.ty.clone())).collect(),target).unwrap();
        assert_eq!((union.size_bytes(target),union.align_bytes(target)),(Some(4),Some(4)));
        assert_eq!(CType::Pointer(Box::new(CType::int())).size_bytes(Target::default()),Some(8));
    }

    #[test]
    fn invalid_target_is_diagnostic() {
        assert!(Target { int_bits: 8, ..Target::default() }.validate().is_err());
    }
    #[test]
    fn recursive_composite_canonicalizes_completed_tag_types() {
        let incomplete = CType::TagRef { kind: TagKind::Struct, name: "Node".into(), identity: 42 };
        let complete = CType::Struct { identity: 42, tag: Some("Node".into()),
            members: vec![], size: 8, align: 8 };
        let old = CType::Pointer(Box::new(incomplete.clone()));
        let new = CType::Pointer(Box::new(complete.clone()));
        assert_eq!(old.composite(&new), Some(new.clone()));
        assert_eq!(new.composite(&old), Some(new.clone()));
        let old_array = CType::Array(Box::new(incomplete), None);
        let new_array = CType::Array(Box::new(complete), Some(3));
        assert_eq!(old_array.composite(&new_array), Some(new_array.clone()));
        assert_eq!(new_array.composite(&old_array), Some(new_array));
    }

}

#[cfg(test)]
mod flexible_array_layout_tests {
    use super::*;

    #[test]
    fn trailing_incomplete_array_layout() {
        let target = Target::default();
        let array = CType::Array(Box::new(CType::int()), None);
        let ty = layout_struct(None, vec![("n".into(), CType::int()), ("data".into(), array.clone())], target).unwrap();
        assert_eq!(ty.size_bytes(target), Some(4));
        assert_eq!(ty.align_bytes(target), Some(4));
        if let CType::Struct { members, .. } = ty {
            assert_eq!(members[1].offset, 4);
            assert_eq!(members[1].ty, array);
        } else { panic!("expected structure"); }
        assert!(layout_struct(None, vec![("data".into(), array.clone())], target).is_err());
        assert!(layout_struct(None, vec![("data".into(), array.clone()), ("n".into(), CType::int())], target).is_err());
        assert!(layout_union(None, vec![("n".into(), CType::int()), ("data".into(), array)], target).is_err());
    }
}
