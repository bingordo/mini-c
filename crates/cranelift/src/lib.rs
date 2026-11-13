//! Optional, backend-only Cranelift object emitter for typed Mini-C functions.
//! No dependency from syntax or sema to Cranelift, and no TU-sized AST.
//!
//! Supports native x86-64 ELF functions, stack objects, and integer-initialized
//! or zero-initialized global objects. Unsupported features fail explicitly.
use cranelift_codegen::ir::{self, condcodes::IntCC, types, AbiParam, InstBuilder, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{default_libcall_names, DataId, FuncId, Linkage as ClLinkage, Module};
use cranelift_module::DataDescription;
use cranelift_object::{ObjectBuilder, ObjectModule};
use object::{RelocationEncoding, RelocationFlags, RelocationKind, SymbolScope};
use object::write::{Relocation, StandardSection};
use mini_c_sema::{
    CType, CheckedDecl, CheckedExpr, CheckedExprKind, CheckedFunction,
    CheckedItem, CheckedStmt, Checker, IntRank, Linkage, Target,
};
use mini_c_syntax::{ast::{BinOp, UnOp, StorageClass}, parser::Parser, stream::ItemSource};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod sysv_aggregate;

type Result<T> = std::result::Result<T, String>;

fn scalar_type(ty: &CType) -> Result<ir::Type> {
    // Object qualifiers affect modifiability, not physical ABI storage.
    match ty.unqualified() {
        CType::Integer { rank: IntRank::Bool | IntRank::Char, .. } => Ok(types::I8),
        CType::Integer { rank: IntRank::Short, .. } => Ok(types::I16),
        CType::Integer { rank: IntRank::Int, .. } => Ok(types::I32),
        CType::Integer { rank: IntRank::Long | IntRank::LongLong, .. } | CType::Pointer(_) => Ok(types::I64),
        CType::Struct { .. } | CType::Union { .. } | CType::TagRef { .. } |
        CType::Array(..) => Err(format!(
            "unsupported SysV aggregate-by-value ABI for {ty:?}; use pointer parameters/returns"
        )),
        _ => Err(format!("unsupported scalar type {ty:?}")),
    }
}

fn signature(module: &ObjectModule, ty: &CType) -> Result<ir::Signature> {
    let (params, result) = match ty {
        CType::Function { params, result } |
        CType::VariadicFunction { params, result } => (params, result),
        _ => return Err("function declaration requires a function type".into()),
    };
    let mut sig = module.make_signature();
    for param in params {
        sig.params.push(AbiParam::new(scalar_type(param)?));
    }
    if result.unqualified() != &CType::Void {
        sig.returns.push(AbiParam::new(scalar_type(result)?));
    }
    Ok(sig)
}

#[derive(Clone)]
struct KnownFunction {
    id: FuncId,
    ty: CType,
    /// Linker-facing symbol; lookup in this map still uses the C identifier.
    link_name: String,
    defined: bool,
}

/// A callsite-specific fixed-ABI wrapper, emitted as a tiny x86-64 tail jump.
/// Its entry sees the normal SysV GP/stack arguments and sets AL=0 before
/// jumping into the real C variadic function. No SSE arguments are supported.
struct SysvGpVarargThunk {
    wrapper: FuncId,
    target: FuncId,
}

/// Fully checked external function prototype waiting for its first
/// code-generation boundary. This permits later GNU asm declarations to
/// finalize the actual ELF symbol name before registration with Cranelift.
struct PendingFunction {
    ty: CType,
    link_name: String,
    linkage: Linkage,
}

#[derive(Clone)]
enum RelocationSource {
    Object(String, i64),
    Literal(Vec<u8>),
}

#[derive(Clone)]
enum RelocationTarget {
    Object(String, i64),
    Literal(DataId),
}

struct KnownData {
    id: DataId,
    ty: CType,
    linkage: Linkage,
    initialized: Option<Vec<u8>>,
    address_of: Option<(String, i64)>,
    string_address: Option<DataId>,
    /// Multiple pointer fields can carry independent ELF data relocations.
    aggregate_relocations: Vec<(u32, RelocationTarget)>,
    tentative: bool,
}

/// Intern narrow string data in this object. Sharing identical literals is permitted
/// by C, and a separate local ELF symbol avoids collisions with C identifiers.
/// The checked IR keeps a narrow string as a byte array, rather than
/// decaying it to a pointer. C permits omitting the final NUL only when the
/// declared array bound exactly fits the nonterminating characters.
fn char_array_initializer_bytes(
    ty: &CType, initializer: &CheckedExpr,
) -> Result<Option<Vec<u8>>> {
    let (CType::Array(element, Some(bound)), CheckedExprKind::StringLiteral(source)) =
        (ty, &initializer.kind)
    else { return Ok(None); };
    if !matches!(element.unqualified(), CType::Integer { rank: IntRank::Char, .. }) {
        return Err("string initializer requires an array of character type".into());
    }
    let count = usize::try_from(*bound).map_err(|_| "character array exceeds host size")?;
    if count > 1_048_576 {
        return Err("character-array initializer exceeds 1 MiB codegen limit".into());
    }
    if count < source.len().saturating_sub(1) {
        return Err("string initializer exceeds character array bound".into());
    }
    let mut bytes = vec![0u8; count];
    let copy = count.min(source.len());
    bytes[..copy].copy_from_slice(&source[..copy]);
    Ok(Some(bytes))
}

/// Resolve a fully checked initializer path to the target aggregate
/// subobject and its byte displacement. Both global packing and stack
/// initialization use this to keep mixed struct/array designators identical.
fn initializer_subobject(
    ty: &CType, path: &[usize],
) -> Result<(CType, u64, Option<mini_c_sema::types::BitField>)> {
    let mut current = ty;
    let mut offset = 0u64;
    let mut bits = None;
    for &index in path {
        let (next, delta, field_bits) = match current.unqualified() {
            CType::Struct { members, .. } | CType::Union { members, .. } => {
                let field = members.get(index).ok_or("invalid checked initializer field")?;
                (&field.ty, field.offset, field.bit_field)
            }
            CType::Array(element, Some(bound)) => {
                if u64::try_from(index).map_or(true, |i| i >= *bound) {
                    return Err("invalid checked initializer array index".into());
                }
                let stride = element.size_bytes(Target::default())
                    .ok_or("incomplete nested initializer element")?;
                (element.as_ref(), u64::try_from(index)
                    .map_err(|_| "nested initializer array index too large")?
                    .checked_mul(stride)
                    .ok_or("nested initializer array offset overflow")?, None)
            }
            _ => return Err("nested initializer path enters a nonaggregate".into()),
        };
        offset = offset.checked_add(delta)
            .ok_or("nested initializer byte offset overflow")?;
        current = next;
        bits = field_bits;
    }
    Ok((current.clone(), offset, bits))
}

/// Resolve a checked constant-address lvalue, preserving an optional
/// struct/union member displacement for ELF data relocations. Arrays decay
/// to their first element (zero displacement). Only the checker can permit
/// these operands into a static pointer initializer.
fn static_object_address(expr: &CheckedExpr) -> Option<(String, i64)> {
    match &expr.kind {
        CheckedExprKind::Name(name) => Some((name.clone(), 0)),
        CheckedExprKind::StaticCompoundLiteral { symbol, .. } =>
            Some((symbol.clone(), 0)),
        CheckedExprKind::Member(base, offset) => {
            let (name, base_offset) = static_object_address(base)?;
            let offset = i64::try_from(*offset).ok()?;
            Some((name, base_offset.checked_add(offset)?))
        }
        _ => None,
    }
}

/// Flatten a checked aggregate constant initializer into ELF bytes.
/// Dynamic nonconstant expressions and objects exceeding the codegen guard
/// are rejected explicitly. Relocations remain separate from raw bytes.
type StaticAggregate = (Vec<u8>, Vec<(u32, RelocationSource)>);

fn static_initializer_bytes(
    ty: &CType, init: &CheckedExpr,
) -> Result<Option<StaticAggregate>> {
    if !matches!(&init.kind,
        CheckedExprKind::Initializer(_) | CheckedExprKind::SparseInitializer(_)
        | CheckedExprKind::DeepInitializer(_))
    { return Ok(None); }
    let size = ty.size_bytes(Target::default()).ok_or("incomplete aggregate initializer")?;
    let size = usize::try_from(size).map_err(|_| "oversized aggregate initializer")?;
    if size > 1_048_576 { return Err("aggregate initializer exceeds 1 MiB codegen limit".into()); }
    let mut bytes = vec![0u8; size];
    let mut relocations = Vec::new();

    /// Insert a constant field value into its shared 32-bit little-endian
    /// storage word; do not overwrite adjacent bit-fields or padding.
    fn fill_bits(
        out: &mut [u8], at: usize, info: mini_c_sema::types::BitField,
        init: &CheckedExpr,
    ) -> Result<()> {
        let end = at.checked_add(4).ok_or("bit-field offset overflow")?;
        let bytes = out.get_mut(at..end)
            .ok_or("bit-field storage extends beyond aggregate")?;
        let value = mini_c_sema::const_eval::evaluate(init, Target::default())
            .map_err(|why| format!("nonconstant bit-field initializer: {why}"))?;
        let mask = if info.width == 32 {
            u32::MAX
        } else {
            (1u32 << info.width) - 1
        };
        let field_mask = mask << info.shift;
        let mut previous = [0u8; 4];
        previous.copy_from_slice(bytes);
        let old = u32::from_le_bytes(previous);
        let packed = (old & !field_mask)
            | (((value as u32) & mask) << info.shift);
        bytes.copy_from_slice(&packed.to_le_bytes());
        Ok(())
    }

    fn fill(
        out: &mut [u8], relocations: &mut Vec<(u32, RelocationSource)>,
        ty: &CType, init: &CheckedExpr, offset: usize,
    ) -> Result<()> {
        let size = usize::try_from(ty.size_bytes(Target::default())
            .ok_or("incomplete initialized object")?)
            .map_err(|_| "oversized initialized object")?;
        let end = offset.checked_add(size).ok_or("initializer byte offset overflow")?;
        if end > out.len() { return Err("initializer extends beyond object bounds".into()); }
        match (&init.kind, ty.unqualified()) {
            (CheckedExprKind::Initializer(values), CType::Struct { members, .. }) |
            (CheckedExprKind::Initializer(values), CType::Union { members, .. }) => {
                for (value, member) in values.iter().zip(members) {
                    let displacement = usize::try_from(member.offset)
                        .map_err(|_| "oversized struct member offset")?;
                    let at = offset.checked_add(displacement)
                        .ok_or("struct member offset overflow")?;
                    if let Some(info) = member.bit_field {
                        fill_bits(out, at, info, value)?;
                    } else {
                        fill(out, relocations, &member.ty, value, at)?;
                    }
                }
            }
            (CheckedExprKind::Initializer(values), CType::Array(element, Some(_))) => {
                let stride = usize::try_from(element.size_bytes(Target::default())
                    .ok_or("incomplete array element")?).map_err(|_| "oversized array element")?;
                for (index, value) in values.iter().enumerate() {
                    let displacement = index.checked_mul(stride).ok_or("array offset overflow")?;
                    fill(out, relocations, element, value,
                        offset.checked_add(displacement).ok_or("array offset overflow")?)?;
                }
            }
            (CheckedExprKind::SparseInitializer(entries),
             CType::Struct { members, .. } | CType::Union { members, .. }) => {
                for (index, value) in entries {
                    let member = members.get(*index)
                        .ok_or("invalid checked initializer member index")?;
                    let displacement = usize::try_from(member.offset)
                        .map_err(|_| "oversized aggregate member offset")?;
                    let at = offset.checked_add(displacement)
                        .ok_or("aggregate member offset overflow")?;
                    if let Some(info) = member.bit_field {
                        fill_bits(out, at, info, value)?;
                    } else {
                        fill(out, relocations, &member.ty, value, at)?;
                    }
                }
            }
            (CheckedExprKind::SparseInitializer(entries),
             CType::Array(element, Some(_))) => {
                let stride = usize::try_from(element.size_bytes(Target::default())
                    .ok_or("incomplete array element")?).map_err(|_| "oversized array element")?;
                for (index, value) in entries {
                    let displacement = index.checked_mul(stride).ok_or("array offset overflow")?;
                    fill(out, relocations, element, value,
                        offset.checked_add(displacement).ok_or("array offset overflow")?)?;
                }
            }
            (CheckedExprKind::DeepInitializer(entries), _) => {
                for (path, value) in entries {
                    let (element, displacement, bits) = initializer_subobject(ty, path)?;
                    let displacement = usize::try_from(displacement)
                        .map_err(|_| "oversized nested initializer byte offset")?;
                    let at = offset.checked_add(displacement)
                        .ok_or("nested initializer byte offset overflow")?;
                    if let Some(info) = bits {
                        fill_bits(out, at, info, value)?;
                    } else {
                        fill(out, relocations, &element, value, at)?;
                    }
                }
            }
            _ => {
                if let Some(string) = char_array_initializer_bytes(ty, init)? {
                    out[offset..end].copy_from_slice(&string);
                } else if ty.is_integer() {
                    let value = mini_c_sema::const_eval::evaluate(init, Target::default())
                        .map_err(|e| format!("nonconstant aggregate initializer: {e}"))?;
                    out[offset..end].copy_from_slice(&value.to_le_bytes()[..size]);
                } else if matches!(ty.unqualified(), CType::Pointer(_)) {
                    let mut source = init;
                    while let CheckedExprKind::Convert(inner) = &source.kind { source = inner; }
                    if mini_c_sema::const_eval::evaluate(source, Target::default()) == Ok(0) {
                        out[offset..end].fill(0);
                    } else {
                        let target = match &source.kind {
                            CheckedExprKind::Unary(UnOp::Address, inner) |
                            CheckedExprKind::ArrayDecay(inner) => inner.as_ref(),
                            _ => return Err("aggregate pointer initializer requires constant address".into()),
                        };
                        let target = if let Some((name, addend)) =
                            static_object_address(target)
                        {
                            RelocationSource::Object(name, addend)
                        } else if let CheckedExprKind::StringLiteral(bytes) = &target.kind {
                            RelocationSource::Literal(bytes.clone())
                        } else {
                            return Err("aggregate pointer initializer requires constant object address".into());
                        };
                        let offset = u32::try_from(offset)
                            .map_err(|_| "aggregate relocation offset overflow")?;
                        relocations.push((offset, target));
                    }
                } else {
                    return Err("unsupported aggregate constant initializer".into());
                }
            }
        }
        Ok(())
    }
    fill(&mut bytes, &mut relocations, ty, init, 0)?;
    Ok(Some((bytes, relocations)))
}

fn intern_string(
    module: &mut ObjectModule,
    strings: &mut HashMap<Vec<u8>, DataId>,
    bytes: &[u8],
) -> Result<DataId> {
    if let Some(&id) = strings.get(bytes) {
        return Ok(id);
    }
    let name = format!(".Lmini_c_string_{}", strings.len());
    let id = module.declare_data(&name, ClLinkage::Local, false, false)
        .map_err(|e| format!("declaring string literal: {e}"))?;
    let mut desc = DataDescription::new();
    desc.define(bytes.to_vec().into_boxed_slice());
    desc.set_align(1);
    module.define_data(id, &desc)
        .map_err(|e| format!("defining string literal: {e}"))?;
    strings.insert(bytes.to_vec(), id);
    Ok(id)
}

/// One object module is built incrementally. Completed checked functions are
/// compiled and discarded immediately; only named signatures/FuncIds survive.
pub struct Compiler {
    module: ObjectModule,
    functions: HashMap<String, KnownFunction>,
    pending_functions: HashMap<String, PendingFunction>,
    gp_vararg_thunks: Vec<SysvGpVarargThunk>,
    data: HashMap<String, KnownData>,
    strings: HashMap<Vec<u8>, DataId>,
}

impl Compiler {
    pub fn new() -> Result<Self> {
        if !cfg!(all(target_arch = "x86_64", target_os = "linux")) {
            return Err("M2.5 currently supports native x86-64 Linux ELF only".into());
        }
        let mut flags = settings::builder();
        flags.set("is_pic", "true").map_err(|e| e.to_string())?;
        let isa = cranelift_native::builder()
            .map_err(|e| format!("native Cranelift target: {e}"))?
            .finish(settings::Flags::new(flags))
            .map_err(|e| e.to_string())?;
        let object = ObjectBuilder::new(isa, "mini-c", default_libcall_names())
            .map_err(|e| e.to_string())?;
        Ok(Self {
            module: ObjectModule::new(object),
            functions: HashMap::new(), pending_functions: HashMap::new(),
            gp_vararg_thunks: Vec::new(),
            data: HashMap::new(), strings: HashMap::new(),
        })
    }

    fn declare(&mut self, name: &str, ty: &CType, linkage: Linkage, definition: bool,
        asm_label: Option<&str>) -> Result<FuncId> {
        // glibc can first declare an ordinary function and later attach a
        // GNU assembler linkage label to a compatible redeclaration. Before
        // any function bodies have been emitted, the old imported symbol has
        // no code references, so it is safe to bind the source identifier to
        // a new FuncId with the actual ELF symbol name. Deferred global data
        // relocations resolve through this map at finish().
        //
        // Once any body has been emitted, do not silently retarget previously
        // generated call relocations. That uncommon ordering fails closed.
        if let (Some(label), Some(prior)) = (asm_label, self.functions.get(name)) {
            if label != prior.link_name {
                if definition || prior.link_name != name || prior.defined
                    || self.functions.values().any(|entry| entry.defined)
                {
                    return Err(format!(
                        "conflicting assembler linkage name for '{name}' after native code emission"
                    ));
                }
                let composite = prior.ty.composite(ty)
                    .ok_or_else(|| format!("conflicting backend signature for '{name}'"))?;
                let sig = signature(&self.module, &composite)?;
                let id = self.module.declare_function(label, ClLinkage::Import, &sig)
                    .map_err(|e| format!("declaring '{name}' as '{label}': {e}"))?;
                self.functions.insert(name.to_owned(), KnownFunction {
                    id, ty: composite, link_name: label.to_owned(), defined: false,
                });
                return Ok(id);
            }
        }
        let composite = if let Some(known) = self.functions.get_mut(name) {
            if asm_label.is_some_and(|label| label != known.link_name) {
                return Err(format!("conflicting assembler linkage name for '{name}'"));
            }
            let composite = known.ty.composite(ty)
                .ok_or_else(|| format!("conflicting backend signature for '{name}'"))?;
            if definition && known.defined {
                return Err(format!("duplicate backend definition for '{name}'"));
            }
            known.ty = composite.clone();
            if !definition {
                return Ok(known.id);
            }
            composite
        } else {
            ty.clone()
        };
        let linkage = match (linkage, definition) {
            (Linkage::Internal, _) => ClLinkage::Local,
            (_, true) => ClLinkage::Export,
            (_, false) => ClLinkage::Import,
        };
        let sig = signature(&self.module, &composite)?;
        let link_name = asm_label.unwrap_or(name);
        let id = self.module.declare_function(link_name, linkage, &sig)
            .map_err(|e| format!("declaring '{name}' as '{link_name}': {e}"))?;
        self.functions.insert(name.to_owned(), KnownFunction {
            id, ty: composite, link_name: link_name.to_owned(), defined: definition,
        });
        Ok(id)
    }

    /// Discover anonymous static compound literals in a checked initializer
    /// before packing bytes/relocations for the containing global. Each
    /// expression occurrence receives one private data symbol from sema,
    /// independent of identical initializer contents or other declarations.
    fn register_static_compounds(&mut self, expr: &CheckedExpr) -> Result<()> {
        match &expr.kind {
            CheckedExprKind::StaticCompoundLiteral { symbol, initializer } => {
                if !self.data.contains_key(symbol) {
                    self.declare_object(CheckedDecl {
                        name: symbol.clone(),
                        storage: StorageClass::Static,
                        ty: expr.ty.clone(),
                        linkage: Linkage::Internal,
                        asm_label: None,
                        initializer: Some((**initializer).clone()),
                        span: Default::default(),
                    })?;
                }
            }
            CheckedExprKind::Initializer(values) => {
                for value in values { self.register_static_compounds(value)?; }
            }
            CheckedExprKind::SparseInitializer(values) => {
                for (_, value) in values { self.register_static_compounds(value)?; }
            }
            CheckedExprKind::DeepInitializer(values) => {
                for (_, value) in values { self.register_static_compounds(value)?; }
            }
            CheckedExprKind::ArrayDecay(inner) |
            CheckedExprKind::Unary(_, inner) |
            CheckedExprKind::Convert(inner) |
            CheckedExprKind::Load(inner) |
            CheckedExprKind::Member(inner, _) =>
                self.register_static_compounds(inner)?,
            _ => {}
        }
        Ok(())
    }

    fn declare_object(&mut self, decl: CheckedDecl) -> Result<()> {
        if let Some(init) = &decl.initializer {
            self.register_static_compounds(init)?;
        }
        let name = decl.name.clone();
        let mut address_of = None;
        let mut string_address = None;
        let mut aggregate_relocations = Vec::new();
        let initialized = if let Some(init) = &decl.initializer {
            if let Some(bytes) = char_array_initializer_bytes(&decl.ty, init)? {
                Some(bytes)
            } else if let Some((bytes, relocations)) =
                static_initializer_bytes(&decl.ty, init)?
            {
                for (offset, source) in relocations {
                    let target = match source {
                        RelocationSource::Object(name, addend) =>
                            RelocationTarget::Object(name, addend),
                        RelocationSource::Literal(bytes) => RelocationTarget::Literal(
                            intern_string(&mut self.module, &mut self.strings, &bytes)?),
                    };
                    aggregate_relocations.push((offset, target));
                }
                Some(bytes)
            } else {
            if !decl.ty.is_integer() && !matches!(decl.ty.unqualified(), CType::Pointer(_)) {
                return Err(format!("global initializer for '{name}' requires scalar support"));
            }
            let value = if matches!(decl.ty.unqualified(), CType::Pointer(_)) {
                // The checker has already validated the null-pointer constant.
                // Address-of-symbol initializers require data relocations.
                let mut integer = init;
                while let CheckedExprKind::Convert(inner) = &integer.kind {
                    integer = inner;
                }
                match mini_c_sema::const_eval::evaluate(integer, Target::default()) {
                    Ok(0) => 0,
                    _ => {
                        let target = match &integer.kind {
                            CheckedExprKind::Unary(UnOp::Address, target)
                            | CheckedExprKind::ArrayDecay(target) => Some(target.as_ref()),
                            _ => None,
                        };
                        if let Some((symbol, addend)) =
                            target.and_then(static_object_address)
                        {
                            address_of = Some((symbol, addend));
                            0
                        } else if let Some(CheckedExprKind::StringLiteral(bytes)) =
                            target.map(|expr| &expr.kind)
                        {
                            string_address = Some(intern_string(&mut self.module, &mut self.strings, bytes)?);
                            0
                        } else {
                            return Err(format!("global initializer for '{name}' requires a null pointer or constant object address"));
                        }
                    }
                }
            } else {
                mini_c_sema::const_eval::evaluate(init, Target::default())
                    .map_err(|e| format!("global initializer for '{name}': {e}"))?
            };
            let size = decl.ty.size_bytes(Target::default())
                .ok_or_else(|| format!("incomplete global object '{name}'"))?;
            let bytes = value.to_le_bytes();
            Some(bytes[..usize::try_from(size).map_err(|_| "oversized global object")?].to_vec())
            }
        } else { None };
        let tentative = decl.storage != StorageClass::Extern && initialized.is_none();
        if let Some(data) = self.data.get_mut(&name) {
            let Some(composite) = data.ty.composite(&decl.ty) else {
                return Err(format!("conflicting global object declaration '{name}'"));
            };
            if data.linkage != decl.linkage {
                return Err(format!("conflicting global object linkage '{name}'"));
            }
            data.ty = composite;
            if initialized.is_some() && data.initialized.is_some() {
                return Err(format!("duplicate global object definition '{name}'"));
            }
            if initialized.is_some() {
                data.initialized = initialized;
                data.address_of = address_of;
                data.string_address = string_address;
                data.aggregate_relocations = aggregate_relocations;
            }
            data.tentative |= tentative;
            if data.initialized.is_some() || data.tentative {
                let linkage = if data.linkage == Linkage::Internal {
                    ClLinkage::Local
                } else {
                    ClLinkage::Export
                };
                self.module.declare_data(&name, linkage, true, false)
                    .map_err(|e| format!("upgrading global '{name}': {e}"))?;
            }
            return Ok(());
        }
        // A declaration-only external object is undefined in this translation
        // unit. It must remain an import unless a definition appears later.
        let linkage = if decl.linkage == Linkage::Internal {
            ClLinkage::Local
        } else if initialized.is_some() || tentative {
            ClLinkage::Export
        } else {
            ClLinkage::Import
        };
        let id = self.module.declare_data(&name, linkage, true, false)
            .map_err(|e| format!("declaring global '{name}': {e}"))?;
        self.data.insert(name, KnownData {
            id, ty: decl.ty, linkage: decl.linkage, initialized,
            address_of, string_address, aggregate_relocations, tentative,
        });
        Ok(())
    }

    /// Imported prototypes are semantically checked as they stream in, but
    /// an external ELF name is committed only before code emission. In glibc
    /// a compatible later declaration may attach GNU __asm__("symbol"), and
    /// registering the earlier source name immediately would mislink calls.
    fn flush_pending_imports(&mut self) -> Result<()> {
        let mut names = self.pending_functions.keys().cloned().collect::<Vec<_>>();
        names.sort();
        for name in names {
            let pending = self.pending_functions.remove(&name).expect("pending import");
            self.declare(&name, &pending.ty, pending.linkage, false,
                Some(pending.link_name.as_str()))?;
        }
        Ok(())
    }

    /// Register declarations and compile one complete checked function.
    pub fn add(&mut self, item: CheckedItem) -> Result<()> {
        match item {
            CheckedItem::Typedef { .. } | CheckedItem::Tag => Ok(()),
            CheckedItem::Declaration(decl) => {
                if matches!(decl.ty, CType::Function { .. } | CType::VariadicFunction { .. }) {
                    // Header prototypes do not need an emitted ABI signature
                    // until they are used. In particular, glibc stdio.h
                    // declares unused functions with by-value aggregate or
                    // floating-point parameters that this backend cannot yet
                    // lower. The semantic checker has already validated them.
                    //
                    // Do not register a fake signature: an actual call or
                    // function-address reference will fail explicitly because
                    // there is no native declaration. Definitions still go
                    // through define() and must have a supported ABI.
                    match signature(&self.module, &decl.ty) {
                        Ok(_) => {}
                        Err(reason) if reason.starts_with("unsupported SysV aggregate-by-value ABI")
                            || reason.starts_with("unsupported scalar type") => return Ok(()),
                        Err(reason) => return Err(reason),
                    }
                    let name = decl.name.clone();
                    let link_name = decl.asm_label.unwrap_or_else(|| name.clone());
                    if self.functions.contains_key(&name) {
                        // Previously used or defined imports cannot be renamed;
                        // the backend refuses an incompatible late label.
                        self.declare(&name, &decl.ty, decl.linkage, false,
                            Some(link_name.as_str()))?;
                    } else if let Some(pending) = self.pending_functions.get_mut(&name) {
                        pending.ty = pending.ty.composite(&decl.ty)
                            .ok_or_else(|| format!("conflicting function prototype for '{name}'"))?;
                        pending.link_name = link_name;
                        pending.linkage = decl.linkage;
                    } else {
                        self.pending_functions.insert(name, PendingFunction {
                            ty: decl.ty, link_name, linkage: decl.linkage,
                        });
                    }
                    Ok(())
                } else { self.declare_object(decl) }
            }
            CheckedItem::Function(function) => self.define(function),
        }
    }

    fn collect_goto_targets(stmt: &CheckedStmt, targets: &mut Vec<String>) {
        match stmt {
            CheckedStmt::Goto { target } => targets.push(target.clone()),
            CheckedStmt::Label { body, .. } => Self::collect_goto_targets(body, targets),
            CheckedStmt::Block(stmts) => {
                for stmt in stmts {
                    Self::collect_goto_targets(stmt, targets);
                }
            }
            CheckedStmt::If { then, otherwise, .. } => {
                Self::collect_goto_targets(then, targets);
                if let Some(otherwise) = otherwise {
                    Self::collect_goto_targets(otherwise, targets);
                }
            }
            CheckedStmt::While { body, .. } | CheckedStmt::DoWhile { body, .. }
            | CheckedStmt::Switch { body, .. } | CheckedStmt::Case { body, .. }
            | CheckedStmt::Default(body) => Self::collect_goto_targets(body, targets),
            CheckedStmt::For(for_loop) => {
                if let Some(init) = &for_loop.init {
                    Self::collect_goto_targets(init, targets);
                }
                Self::collect_goto_targets(&for_loop.body, targets);
            }
            _ => {}
        }
    }

    /// Preallocate ordinary C label blocks in lexical source order, even when
    /// they are nested in switch arms and their enclosing control constructs
    /// have no ordinary incoming predecessor. Switch case/default labels are
    /// planned separately by the innermost switch.
    fn collect_native_labels(stmt: &CheckedStmt, ordered: &mut Vec<String>) -> Result<()> {
        match stmt {
            CheckedStmt::Label { name, body } => {
                ordered.push(name.clone());
                Self::collect_native_labels(body, ordered)?;
            }
            CheckedStmt::Block(stmts) => {
                for stmt in stmts {
                    Self::collect_native_labels(stmt, ordered)?;
                }
            }
            CheckedStmt::If { then, otherwise, .. } => {
                Self::collect_native_labels(then, ordered)?;
                if let Some(otherwise) = otherwise {
                    Self::collect_native_labels(otherwise, ordered)?;
                }
            }
            CheckedStmt::While { body, .. } | CheckedStmt::DoWhile { body, .. } =>
                Self::collect_native_labels(body, ordered)?,
            CheckedStmt::For(for_loop) => {
                if let Some(init) = &for_loop.init {
                    Self::collect_native_labels(init, ordered)?;
                }
                Self::collect_native_labels(&for_loop.body, ordered)?;
            }
            CheckedStmt::Switch { body, .. } | CheckedStmt::Case { body, .. }
            | CheckedStmt::Default(body) => Self::collect_native_labels(body, ordered)?,
            _ => {}
        }
        Ok(())
    }

    fn native_goto_plan(body: &CheckedStmt) -> Result<(Vec<String>, HashSet<String>)> {
        if !matches!(body, CheckedStmt::Block(_)) {
            return Err("native goto requires a compound function body".into());
        }
        let mut ordered_labels = Vec::new();
        Self::collect_native_labels(body, &mut ordered_labels)?;
        let definitions: HashSet<_> = ordered_labels.iter().cloned().collect();
        let mut targets = Vec::new();
        Self::collect_goto_targets(body, &mut targets);
        let mut referenced = HashSet::new();
        for target in targets {
            if !definitions.contains(&target) {
                return Err(format!("native goto refers to missing checked label '{target}'"));
            }
            referenced.insert(target);
        }
        // Source order is deterministic; hash-map order is not.
        Ok((ordered_labels, referenced))
    }

    /// Report whether a CFG block is reachable from the real function entry.
    /// Synthetic entries used to represent goto/case targets are deliberately
    /// disconnected; lexical fallthrough from them must not make a non-void
    /// function appear to fall off the end.
    fn block_reachable_from_entry(
        function: &ir::Function, entry: ir::Block, target: ir::Block,
    ) -> bool {
        let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(function);
        let mut visited = HashSet::new();
        let mut worklist = vec![entry];
        while let Some(block) = worklist.pop() {
            if !visited.insert(block) { continue; }
            if block == target { return true; }
            worklist.extend(cfg.succ_iter(block));
        }
        false
    }

    fn define(&mut self, function: CheckedFunction) -> Result<()> {
        let (label_names, referenced_labels) = Self::native_goto_plan(&function.body)?;
        self.flush_pending_imports()?;
        let id = self.declare(&function.name, &function.signature, function.linkage, true, None)?;
        let mut ctx = self.module.make_context();
        ctx.func.signature = signature(&self.module, &function.signature)?;
        ctx.func.name = ir::UserFuncName::user(0, id.as_u32());
        let config = self.module.target_config();
        {
            let mut fb_ctx = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);
            let labels = label_names.into_iter()
                .map(|name| (name, builder.create_block()))
                .collect();
            let mut lower = Lower {
                module: &mut self.module, functions: &self.functions,
                gp_vararg_thunks: &mut self.gp_vararg_thunks,
                data: &self.data,
                strings: &mut self.strings,
                scopes: vec![HashMap::new()], loops: Vec::new(), breaks: Vec::new(),
                switch_labels: Vec::new(), labels, referenced_labels,
                result_ty: match &function.signature {
                    CType::Function { result, .. } => result.as_ref().clone(),
                    _ => return Err("function signature expected".into()),
                },
            };
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            builder.seal_block(entry);
            let params = builder.block_params(entry).to_vec();
            if params.len() != function.params.len() {
                return Err(format!("ABI parameter mismatch for '{}'", function.name));
            }
            for (param, value) in function.params.iter().zip(params) {
                lower.bind(&mut builder, &param.name, &param.ty, Some(value))?;
            }
            let live = lower.statement(&mut builder, &function.body)?;
            if live {
                let end = builder.current_block()
                    .ok_or("native lowering has no current end block")?;
                if !Self::block_reachable_from_entry(builder.func, entry, end) {
                    // Lowering a dead if/loop/switch around an externally
                    // referenced label can create a syntactically live exit.
                    // Terminate it for Cranelift, but do not treat it as an
                    // actual missing return from the function entry.
                    builder.ins().trap(ir::TrapCode::unwrap_user(1));
                } else if lower.result_ty == CType::Void {
                    builder.ins().return_(&[]);
                } else if function.name == "main" {
                    // Reaching the closing brace of C main means return 0.
                    let zero = builder.ins().iconst(scalar_type(&lower.result_ty)?, 0);
                    builder.ins().return_(&[zero]);
                } else {
                    return Err(format!("function '{}' may reach its end without a return", function.name));
                }
            }
            builder.seal_all_blocks();
            builder.finalize(config);
        }
        self.module.define_function(id, &mut ctx)
            .map_err(|e| format!("compiling '{}': {e}", function.name))?;
        Ok(())
    }

    /// Resolve late-completed file-scope tags before final ELF data layout.
    /// This is separate from add() so streaming declarations need not retain
    /// the entire translation unit or update prior checked items.
    pub fn complete_file_object_types(&mut self, checker: &Checker) -> Result<()> {
        for (name, data) in &mut self.data {
            // Anonymous C99 compound literals have no source-level name in
            // the checker's global symbol table, and their object type was
            // already completed and validated at the initializer site.
            if name.starts_with(".Lmini_c_compound_") { continue; }
            let resolved = checker.completed_global_type(name)
                .ok_or_else(|| format!("missing semantic declaration for global '{name}'"))?;
            data.ty = data.ty.composite(&resolved)
                .ok_or_else(|| format!("conflicting final global object type for '{name}'"))?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<Vec<u8>> {
        self.flush_pending_imports()?;
        for (name, data) in &self.data {
            if data.initialized.is_none() && !data.tentative { continue; }
            let mut desc = DataDescription::new();
            if let Some(bytes) = &data.initialized {
                desc.define(bytes.clone().into_boxed_slice());
            } else {
                // File-scope tentative arrays with no bound become one-element
                // arrays if no later declaration provided a complete bound.
                let completed = match &data.ty {
                    CType::Array(element, None) if element.size_bytes(Target::default()).is_some() =>
                        CType::Array(element.clone(), Some(1)),
                    other => other.clone(),
                };
                let size = completed.size_bytes(Target::default())
                    .ok_or_else(|| format!("incomplete global object '{name}'"))?;
                let size = usize::try_from(size).map_err(|_| format!("oversized global object '{name}'"))?;
                desc.define_zeroinit(size);
            }
            // Keep global object alignment consistent with stack-object layout.
            // Cranelift expects alignment in bytes, unlike stack-slot log2 alignment.
            let alignment = data.ty.align_bytes(Target::default()).unwrap_or(1);
            desc.set_align(alignment);
            if let Some((symbol, addend)) = &data.address_of {
                if let Some(target) = self.data.get(symbol) {
                    let gv = self.module.declare_data_in_data(target.id, &mut desc);
                    desc.write_data_addr(0, gv, *addend);
                } else if let Some(target) = self.functions.get(symbol) {
                    if *addend != 0 {
                        return Err(format!("function address for '{name}' cannot have data addend"));
                    }
                    let func = self.module.declare_func_in_data(target.id, &mut desc);
                    desc.write_function_addr(0, func);
                } else {
                    return Err(format!("global initializer for '{name}' refers to unknown symbol '{symbol}'"));
                }
            } else if let Some(string) = data.string_address {
                let gv = self.module.declare_data_in_data(string, &mut desc);
                desc.write_data_addr(0, gv, 0);
            }
            for (offset, target) in &data.aggregate_relocations {
                match target {
                    RelocationTarget::Object(symbol, addend) => {
                        if let Some(target) = self.data.get(symbol) {
                            let gv = self.module.declare_data_in_data(target.id, &mut desc);
                            desc.write_data_addr(*offset, gv, *addend);
                        } else if let Some(target) = self.functions.get(symbol) {
                            if *addend != 0 {
                                return Err(format!("function address for '{name}' cannot have data addend"));
                            }
                            let func = self.module.declare_func_in_data(target.id, &mut desc);
                            desc.write_function_addr(*offset, func);
                        } else {
                            return Err(format!("aggregate initializer for '{name}' refers to unknown symbol '{symbol}'"));
                        }
                    }
                    RelocationTarget::Literal(id) => {
                        let gv = self.module.declare_data_in_data(*id, &mut desc);
                        desc.write_data_addr(*offset, gv, 0);
                    }
                }
            }
            self.module.define_data(data.id, &desc)
                .map_err(|e| format!("defining global '{name}': {e}"))?;
        }
        let mut product = self.module.finish();
        if !self.gp_vararg_thunks.is_empty() {
            let text = product.object.section_id(StandardSection::Text);
            for thunk in self.gp_vararg_thunks {
                let wrapper = product.function_symbol(thunk.wrapper);
                let destination = product.function_symbol(thunk.target);
                // x86-64: xor eax,eax ; jmp rel32. The tail jump preserves
                // every incoming GP register, stack argument, return address,
                // and alignment while setting the SysV SSE-register count AL=0.
                // An undefined Cranelift import has Unknown scope in the
                // object writer. Set a concrete local scope before defining
                // the code bytes, as required by object::write.
                product.object.symbol_mut(wrapper).scope = SymbolScope::Compilation;
                let start = product.object.add_symbol_data(
                    wrapper, text, &[0x31, 0xc0, 0xe9, 0, 0, 0, 0], 16
                );
                product.object.add_relocation(text, Relocation {
                    offset: start + 3,
                    symbol: destination,
                    addend: -4,
                    flags: RelocationFlags::Generic {
                        kind: RelocationKind::PltRelative,
                        encoding: RelocationEncoding::X86Branch,
                        size: 32,
                    },
                }).map_err(|e| format!("SysV varargs thunk relocation: {e}"))?;
            }
        }
        product.emit().map_err(|e| e.to_string())
    }
}

/// Compile using the same incremental Parser + Checker as the normal driver.
/// The source string is currently buffered; function ASTs are not retained.
/// Preserves parser and type-checker source spans for drivers that map
/// preprocessor line directives back to the original include file.
#[derive(Debug, Clone)]
pub enum CompileError {
    Diagnostic(mini_c_syntax::diag::Diagnostic),
    Backend(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Diagnostic(d) => f.write_str(&d.message),
            Self::Backend(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CompileError {}

pub fn compile_source_diagnostic(source: &str) -> std::result::Result<Vec<u8>, CompileError> {
    let mut parser = Parser::new(source);
    let mut checker = Checker::new(Target::default())
        .map_err(CompileError::Diagnostic)?;
    let mut compiler = Compiler::new().map_err(CompileError::Backend)?;
    while let Some(item) = parser.next_item().map_err(CompileError::Diagnostic)? {
        let checked = checker.check_item(item).map_err(CompileError::Diagnostic)?;
        compiler.add(checked).map_err(CompileError::Backend)?;
    }
    compiler.complete_file_object_types(&checker).map_err(CompileError::Backend)?;
    compiler.finish().map_err(CompileError::Backend)
}

/// Compatibility entry point for library callers that need a plain error
/// string instead of structured source diagnostics.
pub fn compile_source(source: &str) -> Result<Vec<u8>> {
    compile_source_diagnostic(source).map_err(|e| e.to_string())
}

#[derive(Clone, Copy)]
struct Local {
    /// Stack slot identity, not an SSA stack_addr value tied to a particular
    /// predecessor. Materialize an address at its *use*, after any goto edge.
    slot: ir::StackSlot,
}

struct Lower<'a> {
    module: &'a mut ObjectModule,
    functions: &'a HashMap<String, KnownFunction>,
    gp_vararg_thunks: &'a mut Vec<SysvGpVarargThunk>,
    data: &'a HashMap<String, KnownData>,
    strings: &'a mut HashMap<Vec<u8>, DataId>,
    scopes: Vec<HashMap<String, Local>>,
    loops: Vec<(ir::Block, ir::Block)>, // (break target, continue target)
    /// Nearest breakable construct: a loop *or* switch. Continue uses loops.
    breaks: Vec<(ir::Block, bool)>, // destination, whether a break may reach it
    /// Maps source case/default node identities to the currently active
    /// switch dispatch's blocks; nested switches push their own scope.
    switch_labels: Vec<HashMap<usize, ir::Block>>,
    /// Preallocated function-scope basic blocks for source labels.
    labels: HashMap<String, ir::Block>,
    /// Only these labels can revive code after an unconditional terminator.
    referenced_labels: HashSet<String>,
    result_ty: CType,
}

impl Lower<'_> {
    /// Current x86-64 LP64 object model: use explicit storage for all local
    /// objects so that aliases through &x, p[i], and *p stay coherent.
    fn alignment(ty: &CType) -> u8 {
        match ty.unqualified() {
            CType::Struct { align, .. } | CType::Union { align, .. } =>
                u8::try_from(align.trailing_zeros()).unwrap_or(3),
            CType::Array(element, _) => Self::alignment(element),
            CType::Integer { rank: IntRank::Bool | IntRank::Char, .. } => 0,
            CType::Integer { rank: IntRank::Short, .. } => 1,
            CType::Integer { rank: IntRank::Int, .. } => 2,
            _ => 3,
        }
    }

    /// Allocate a stack slot and bind the source name without emitting an
    /// instruction. This also handles declarations skipped by a forward goto:
    /// a later label can still refer to their storage, but no initializer or
    /// synthetic zero store is executed on the bypassed path.
    fn bind_storage(&mut self, b: &mut FunctionBuilder<'_>, name: &str, ty: &CType) -> Result<()> {
        let size = ty.size_bytes(Target::default())
            .ok_or_else(|| format!("M2.5 cannot allocate incomplete or non-object type {ty:?}"))?;
        let size = u32::try_from(size).map_err(|_| "M2.5 stack object exceeds 4GiB")?;
        let slot = b.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot, size, Self::alignment(ty),
        ));
        self.scopes.last_mut().expect("scope exists").insert(name.to_owned(), Local { slot });
        Ok(())
    }

    fn bind(&mut self, b: &mut FunctionBuilder<'_>, name: &str, ty: &CType, value: Option<Value>) -> Result<()> {
        self.bind_storage(b, name, ty)?;
        // Rematerialized in the current block, never inherited across CFG
        // edges from an earlier declaration or parameter block.
        let address = self.object_address(b, name)?;
        if let Some(value) = value {
            b.ins().store(ir::MemFlagsData::new(), value, address, 0);
        } else if !matches!(ty.unqualified(), CType::Array(..) | CType::Struct { .. } | CType::Union { .. }) {
            // Deterministic choice for otherwise-indeterminate scalar locals,
            // not a promise about C uninitialized reads.
            let zero = b.ins().iconst(scalar_type(ty)?, 0);
            b.ins().store(ir::MemFlagsData::new(), zero, address, 0);
        }
        Ok(())
    }

    /// Initialize an automatic character array in native chunks rather than
    /// issuing one instruction for every byte of a string literal.
    /// Explicit byte-accurate struct copy. Copying the full representation,
    /// including padding, keeps nested aggregates and pointer fields intact.
    /// Source and destination are evaluated once and are independently
    /// addressable. No aggregate ABI lowering is implied.
    fn copy_bytes(
        b: &mut FunctionBuilder<'_>, dst: Value, src: Value, size: u64,
    ) -> Result<()> {
        let size = usize::try_from(size).map_err(|_| "struct copy too large")?;
        let mut offset = 0usize;
        while offset < size {
            let remaining = size - offset;
            let (width, ty) = if remaining >= 8 { (8, types::I64) }
                else if remaining >= 4 { (4, types::I32) }
                else if remaining >= 2 { (2, types::I16) }
                else { (1, types::I8) };
            let disp = i32::try_from(offset).map_err(|_| "struct copy offset too large")?;
            let value = b.ins().load(ty, ir::MemFlagsData::new(), src, disp);
            b.ins().store(ir::MemFlagsData::new(), value, dst, disp);
            offset += width;
        }
        Ok(())
    }

    /// Initialize a checked subobject. A bit-field leaf is a masked write
    /// into its shared 32-bit unit, preserving preceding field values.
    fn initialize_subobject(
        &mut self, b: &mut FunctionBuilder<'_>, address: Value,
        ty: &CType, init: &CheckedExpr,
        bits: Option<mini_c_sema::types::BitField>,
    ) -> Result<()> {
        if let Some(info) = bits {
            let value = self.expression(b, init)?;
            let signed = matches!(ty.unqualified(),
                CType::Integer { unsigned: false, .. });
            Self::store_bit_field(b, address, value, info.shift, info.width, signed);
            Ok(())
        } else {
            self.initialize_object(b, address, ty, init)
        }
    }

    /// Recursively initialize a stack aggregate, including zero-filling all
    /// omitted fields and padding before writing explicit elements.
    fn initialize_object(
        &mut self, b: &mut FunctionBuilder<'_>, address: Value,
        ty: &CType, initializer: &CheckedExpr,
    ) -> Result<()> {
        if let Some(bytes) = char_array_initializer_bytes(ty, initializer)? {
            return Self::store_array_bytes(b, address, &bytes);
        }
        if let CheckedExprKind::DeepInitializer(entries) = &initializer.kind {
            let size = usize::try_from(ty.size_bytes(Target::default())
                .ok_or("incomplete deep aggregate initializer")?)
                .map_err(|_| "oversized deep aggregate initializer")?;
            if size > 1_048_576 {
                return Err("aggregate initializer exceeds 1 MiB codegen limit".into());
            }
            Self::store_array_bytes(b, address, &vec![0u8; size])?;
            for (path, value) in entries {
                let (element, offset, bits) = initializer_subobject(ty, path)?;
                let offset = i64::try_from(offset)
                    .map_err(|_| "nested initializer offset exceeds i64")?;
                let addr = b.ins().iadd_imm_s(address, offset);
                self.initialize_subobject(b, addr, &element, value, bits)?;
            }
            return Ok(());
        }
        if let CheckedExprKind::SparseInitializer(entries) = &initializer.kind {
            let size = usize::try_from(ty.size_bytes(Target::default())
                .ok_or("incomplete sparse aggregate initializer")?)
                .map_err(|_| "oversized sparse aggregate initializer")?;
            if size > 1_048_576 {
                return Err("aggregate initializer exceeds 1 MiB codegen limit".into());
            }
            Self::store_array_bytes(b, address, &vec![0u8; size])?;
            match ty.unqualified() {
                CType::Struct { members, .. } | CType::Union { members, .. } => {
                    for (index, value) in entries {
                        let member = members.get(*index)
                            .ok_or("invalid checked sparse member index")?;
                        let offset = i64::try_from(member.offset)
                            .map_err(|_| "oversized sparse member offset")?;
                        let addr = b.ins().iadd_imm_s(address, offset);
                        self.initialize_subobject(
                            b, addr, &member.ty, value, member.bit_field)?;
                    }
                }
                CType::Array(element, Some(_)) => {
                    let stride = element.size_bytes(Target::default())
                        .ok_or("incomplete sparse array element")?;
                    for (index, value) in entries {
                        let offset = i64::try_from(*index).ok()
                            .and_then(|i| i.checked_mul(i64::try_from(stride).ok()?))
                            .ok_or("oversized sparse array offset")?;
                        let addr = b.ins().iadd_imm_s(address, offset);
                        self.initialize_object(b, addr, element, value)?;
                    }
                }
                _ => return Err("sparse initializer requires aggregate type".into()),
            }
            return Ok(());
        }
        if let CheckedExprKind::Initializer(values) = &initializer.kind {
            let size = usize::try_from(ty.size_bytes(Target::default())
                .ok_or("incomplete aggregate initializer")?)
                .map_err(|_| "oversized aggregate initializer")?;
            if size > 1_048_576 {
                return Err("aggregate initializer exceeds 1 MiB codegen limit".into());
            }
            Self::store_array_bytes(b, address, &vec![0u8; size])?;
            match ty.unqualified() {
                CType::Struct { members, .. } | CType::Union { members, .. } => {
                    for (value, member) in values.iter().zip(members) {
                        let offset = i64::try_from(member.offset)
                            .map_err(|_| "oversized structure offset")?;
                        let addr = b.ins().iadd_imm_s(address, offset);
                        self.initialize_subobject(
                            b, addr, &member.ty, value, member.bit_field)?;
                    }
                }
                CType::Array(element, Some(_)) => {
                    let stride = element.size_bytes(Target::default())
                        .ok_or("incomplete array element")?;
                    for (index, value) in values.iter().enumerate() {
                        let offset = i64::try_from(index).ok()
                            .and_then(|i| i.checked_mul(i64::try_from(stride).ok()?))
                            .ok_or("oversized array offset")?;
                        let addr = b.ins().iadd_imm_s(address, offset);
                        self.initialize_object(b, addr, element, value)?;
                    }
                }
                _ => return Err("unexpected braced initializer for scalar".into()),
            }
            return Ok(());
        }
        let value = self.expression(b, initializer)?;
        if matches!(ty.unqualified(), CType::Struct { .. } | CType::Union { .. }) {
            let size = ty.size_bytes(Target::default())
                .ok_or("incomplete struct initializer")?;
            Self::copy_bytes(b, address, value, size)
        } else {
            b.ins().store(ir::MemFlagsData::new(), value, address, 0);
            Ok(())
        }
    }

    fn store_array_bytes(
        b: &mut FunctionBuilder<'_>, address: Value, bytes: &[u8]
    ) -> Result<()> {
        let mut offset = 0usize;
        while offset < bytes.len() {
            let remaining = bytes.len() - offset;
            let (width, ty) = if remaining >= 8 { (8, types::I64) }
                else if remaining >= 4 { (4, types::I32) }
                else if remaining >= 2 { (2, types::I16) }
                else { (1, types::I8) };
            let mut packed = [0u8; 8];
            packed[..width].copy_from_slice(&bytes[offset..offset + width]);
            let value = b.ins().iconst(ty, i64::from_le_bytes(packed));
            let displacement = i32::try_from(offset)
                .map_err(|_| "local character-array initializer exceeds addressable offset")?;
            b.ins().store(ir::MemFlagsData::new(), value, address, displacement);
            offset += width;
        }
        Ok(())
    }

    fn object_address(&mut self, b: &mut FunctionBuilder<'_>, name: &str) -> Result<Value> {
        if let Some(local) = self.scopes.iter().rev().find_map(|scope| scope.get(name)) {
            return Ok(b.ins().stack_addr(types::I64, local.slot, 0));
        }
        let data = self.data.get(name).ok_or_else(|| format!("unresolved variable '{name}'"))?;
        let gv = self.module.declare_data_in_func(data.id, b.func);
        Ok(b.ins().symbol_value(types::I64, gv))
    }

    /// Load the logical value from a 32-bit little-endian C bit-field.
    /// The signed case sign-extends from its declared width.
    fn load_bit_field(
        b: &mut FunctionBuilder<'_>, address: Value,
        shift: u8, width: u8, signed: bool,
    ) -> Value {
        let raw = b.ins().load(types::I32, ir::MemFlagsData::new(), address, 0);
        let shifted = if shift == 0 { raw } else {
            b.ins().ushr_imm_s(raw, i64::from(shift))
        };
        let mask = if width == 32 { u32::MAX } else { (1u32 << width) - 1 };
        let value = b.ins().band_imm_s(shifted, i64::from(mask));
        if signed && width < 32 {
            let pad = i64::from(32 - width);
            let high = b.ins().ishl_imm_s(value, pad);
            b.ins().sshr_imm_s(high, pad)
        } else {
            value
        }
    }

    /// Read-modify-write one bit-field's containing int unit, preserving
    /// adjacent fields and ordinary bytes in the same word. Return the
    /// *stored and narrowed* logical value, not the untruncated RHS.
    fn store_bit_field(
        b: &mut FunctionBuilder<'_>, address: Value, value: Value,
        shift: u8, width: u8, signed: bool,
    ) -> Value {
        let mask = if width == 32 { u32::MAX } else { (1u32 << width) - 1 };
        let word_mask = mask << shift;
        let old = b.ins().load(types::I32, ir::MemFlagsData::new(), address, 0);
        let cleared = b.ins().band_imm_s(old, i64::from(!word_mask));
        let truncated = b.ins().band_imm_s(value, i64::from(mask));
        let shifted = if shift == 0 { truncated } else {
            b.ins().ishl_imm_s(truncated, i64::from(shift))
        };
        let packed = b.ins().bor(cleared, shifted);
        b.ins().store(ir::MemFlagsData::new(), packed, address, 0);
        // Avoid another memory load, but preserve the sign-extension rules.
        if signed && width < 32 {
            let pad = i64::from(32 - width);
            let high = b.ins().ishl_imm_s(truncated, pad);
            b.ins().sshr_imm_s(high, pad)
        } else { truncated }
    }

    fn bit_field_details(expr: &CheckedExpr) -> Option<(u8, u8, bool)> {
        let CheckedExprKind::BitFieldMember(_, _, shift, width) = &expr.kind else {
            return None;
        };
        let signed = match expr.ty.unqualified() {
            CType::Integer { unsigned, .. } => !unsigned,
            _ => return None,
        };
        Some((*shift, *width, signed))
    }

    fn place(&mut self, b: &mut FunctionBuilder<'_>, expr: &CheckedExpr) -> Result<Value> {
        match &expr.kind {
            CheckedExprKind::Name(name) => self.object_address(b, name),
            CheckedExprKind::StaticCompoundLiteral { .. } =>
                Err("file-scope compound literal cannot become an automatic stack object".into()),
            CheckedExprKind::CompoundLiteral(initializer) => {
                // The C99 object is addressable for the entire enclosing
                // block. Allocate one fixed stack slot at this expression
                // site, then execute its initializer at each evaluation;
                // use site-local stack_addr so no CFG edge reuses an SSA
                // address defined in a different predecessor block.
                let size = expr.ty.size_bytes(Target::default())
                    .ok_or("compound literal requires complete object type")?;
                let size = u32::try_from(size)
                    .map_err(|_| "compound literal stack object exceeds 4GiB")?;
                if size > 1_048_576 {
                    return Err("compound literal exceeds 1 MiB codegen limit".into());
                }
                let slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                    ir::StackSlotKind::ExplicitSlot, size, Self::alignment(&expr.ty),
                ));
                let address = b.ins().stack_addr(types::I64, slot, 0);
                self.initialize_object(b, address, &expr.ty, initializer)?;
                Ok(address)
            }
            CheckedExprKind::Member(base, offset) |
            CheckedExprKind::BitFieldMember(base, offset, ..) => {
                let address = self.place(b, base)?;
                let offset = i64::try_from(*offset)
                    .map_err(|_| "struct member offset exceeds supported address range")?;
                Ok(b.ins().iadd_imm_s(address, offset))
            }
            CheckedExprKind::StringLiteral(bytes) => {
                let id = intern_string(self.module, self.strings, bytes)?;
                let gv = self.module.declare_data_in_func(id, b.func);
                Ok(b.ins().symbol_value(types::I64, gv))
            }
            CheckedExprKind::Unary(UnOp::Deref, pointer) => self.expression(b, pointer),
            _ => Err("M2.5 expected an addressable local or dereference".into()),
        }
    }

    fn integer_value(&mut self, b: &mut FunctionBuilder<'_>, expr: &CheckedExpr) -> Result<Value> {
        if !expr.ty.is_integer() {
            return Err(format!("M2.5 integer expression expected, got {:?}", expr.ty));
        }
        self.expression(b, expr)
    }

    fn condition(&mut self, b: &mut FunctionBuilder<'_>, expr: &CheckedExpr) -> Result<Value> {
        if !expr.ty.is_scalar() {
            return Err(format!("M2.5 scalar condition expected, got {:?}", expr.ty));
        }
        let value = self.expression(b, expr)?;
        let ty = b.func.dfg.value_type(value);
        let zero = b.ins().iconst(ty, 0);
        Ok(b.ins().icmp(IntCC::NotEqual, value, zero))
    }

    fn boolean(b: &mut FunctionBuilder<'_>, value: Value) -> Value {
        let one = b.ins().iconst(types::I32, 1);
        let zero = b.ins().iconst(types::I32, 0);
        b.ins().select(value, one, zero)
    }

    fn convert(b: &mut FunctionBuilder<'_>, value: Value, from: &CType, to: &CType) -> Result<Value> {
        let src_ty = scalar_type(from)?;
        let dst_ty = scalar_type(to)?;
        // _Bool conversion is truth-value normalization, not integer
        // truncation: both 2 and 256 must become 1, never 2 or 0.
        if matches!(to.unqualified(), CType::Integer { rank: IntRank::Bool, .. }) {
            let nonzero = b.ins().icmp_imm_s(IntCC::NotEqual, value, 0);
            let one = b.ins().iconst(types::I8, 1);
            let zero = b.ins().iconst(types::I8, 0);
            return Ok(b.ins().select(nonzero, one, zero));
        }
        if src_ty == dst_ty { return Ok(value); }
        if src_ty.bits() < dst_ty.bits() {
            let signed = matches!(from, CType::Integer { unsigned: false, .. });
            Ok(if signed { b.ins().sextend(dst_ty, value) } else { b.ins().uextend(dst_ty, value) })
        } else {
            Ok(b.ins().ireduce(dst_ty, value))
        }
    }

    /// Byte displacement shared by ordinary pointer arithmetic and pointer
    /// compound assignments. Both call sites evaluate their operands once.
    fn pointer_displacement(
        b: &mut FunctionBuilder<'_>, raw_index: Value,
        offset_ty: &CType, element: &CType,
    ) -> Result<Value> {
        let width = element.size_bytes(Target::default())
            .ok_or("pointer arithmetic requires sized element")?;
        let width = i64::try_from(width).map_err(|_| "pointer element too large")?;
        let index = if b.func.dfg.value_type(raw_index) == types::I64 {
            raw_index
        } else if matches!(offset_ty, CType::Integer { unsigned: true, .. }) {
            b.ins().uextend(types::I64, raw_index)
        } else {
            b.ins().sextend(types::I64, raw_index)
        };
        let scale = b.ins().iconst(types::I64, width);
        Ok(b.ins().imul(index, scale))
    }

    fn pointer_offset(
        &mut self, b: &mut FunctionBuilder<'_>, op: BinOp,
        pointer: &CheckedExpr, offset: &CheckedExpr,
    ) -> Result<Value> {
        let CType::Pointer(element) = &pointer.ty else {
            return Err("malformed checked pointer arithmetic".into());
        };
        let base = self.expression(b, pointer)?;
        let raw_index = self.integer_value(b, offset)?;
        let displacement = Self::pointer_displacement(
            b, raw_index, &offset.ty, element.as_ref())?;
        Ok(match op {
            BinOp::Add => b.ins().iadd(base, displacement),
            BinOp::Sub => b.ins().isub(base, displacement),
            _ => return Err("invalid checked pointer operator".into()),
        })
    }

    fn short_circuit(&mut self, b: &mut FunctionBuilder<'_>, op: BinOp, left: &CheckedExpr, right: &CheckedExpr) -> Result<Value> {
        let first = self.condition(b, left)?;
        let rhs_block = b.create_block();
        let shortcut = b.create_block();
        let join = b.create_block();
        let var = b.declare_var(types::I32);
        if op == BinOp::And {
            b.ins().brif(first, rhs_block, &[], shortcut, &[]);
        } else {
            b.ins().brif(first, shortcut, &[], rhs_block, &[]);
        }
        b.switch_to_block(shortcut);
        let v = b.ins().iconst(types::I32, i64::from(op == BinOp::Or));
        b.def_var(var, v);
        b.ins().jump(join, &[]);
        b.switch_to_block(rhs_block);
        let rhs = self.condition(b, right)?;
        let rhs = Self::boolean(b, rhs);
        b.def_var(var, rhs);
        b.ins().jump(join, &[]);
        b.switch_to_block(join);
        Ok(b.use_var(var))
    }

    fn expression(&mut self, b: &mut FunctionBuilder<'_>, expr: &CheckedExpr) -> Result<Value> {
        match &expr.kind {
            CheckedExprKind::Initializer(_) | CheckedExprKind::SparseInitializer(_)
            | CheckedExprKind::DeepInitializer(_) =>
                Err("braced initializer cannot be evaluated as an expression".into()),
            CheckedExprKind::Literal(n) => Ok(b.ins().iconst(scalar_type(&expr.ty)?, *n as i64)),
            CheckedExprKind::FloatLiteral(_) =>
                Err("native floating-point literal code generation is unsupported".into()),
            CheckedExprKind::EnumConstant(n) => Ok(b.ins().iconst(scalar_type(&expr.ty)?, *n)),
            CheckedExprKind::StringLiteral(_) | CheckedExprKind::Member(..) |
            CheckedExprKind::BitFieldMember(..) => self.place(b, expr),
            CheckedExprKind::StaticCompoundLiteral { .. } =>
                Err("static compound literal may not be evaluated as an automatic expression".into()),
            CheckedExprKind::CompoundLiteral(_) => {
                let address = self.place(b, expr)?;
                if matches!(expr.ty.unqualified(), CType::Array(..) |
                    CType::Struct { .. } | CType::Union { .. })
                {
                    Ok(address)
                } else {
                    Ok(b.ins().load(scalar_type(&expr.ty)?, ir::MemFlagsData::new(), address, 0))
                }
            }
            CheckedExprKind::Name(name) => self.object_address(b, name),
            CheckedExprKind::Load(inner) => {
                let address = self.place(b, inner)?;
                if let Some((shift, width, signed)) = Self::bit_field_details(inner) {
                    return Ok(Self::load_bit_field(b, address, shift, width, signed));
                }
                if matches!(&expr.ty, CType::Struct { .. } | CType::Union { .. }) {
                    // Aggregate values are represented by their source address
                    // until the explicit copy in assignment/initialization.
                    Ok(address)
                } else {
                    Ok(b.ins().load(scalar_type(&expr.ty)?, ir::MemFlagsData::new(), address, 0))
                }
            }
            CheckedExprKind::ArrayDecay(inner) => self.place(b, inner),
            CheckedExprKind::Convert(inner) => {
                let v = self.expression(b, inner)?;
                Self::convert(b, v, &inner.ty, &expr.ty)
            }
            CheckedExprKind::BuiltinCtz(inner) => {
                // GCC defines __builtin_ctz(0) as undefined behavior. The
                // source must guard zero; CLIF ctz is permitted to produce
                // an arbitrary value for that case. Never promise ctz(0)=32.
                if inner.ty != (CType::Integer { rank: IntRank::Int, unsigned: true }) {
                    return Err("__builtin_ctz requires an unsigned int operand".into());
                }
                let value = self.integer_value(b, inner)?;
                if b.func.dfg.value_type(value) != types::I32 {
                    return Err("__builtin_ctz requires 32-bit int on this backend".into());
                }
                Ok(b.ins().ctz(value))
            }
            CheckedExprKind::Unary(op, inner) => {
                if *op == UnOp::Address {
                    if let (CType::Function { .. }, CheckedExprKind::Name(name)) =
                        (&inner.ty, &inner.kind)
                    {
                        let id = self.functions.get(name)
                            .map(|known| known.id)
                            .ok_or_else(|| format!("missing function declaration for '{name}'"))?;
                        let reference = self.module.declare_func_in_func(id, b.func);
                        return Ok(b.ins().func_addr(types::I64, reference));
                    }
                    return self.place(b, inner);
                }
                if *op == UnOp::Deref {
                    return self.expression(b, inner);
                }
                let val = self.integer_value(b, inner)?;
                match op {
                    UnOp::Plus => Ok(val),
                    UnOp::Neg => Ok(b.ins().ineg(val)),
                    UnOp::BitNot => Ok(b.ins().bnot(val)),
                    UnOp::Not => {
                        let ty = b.func.dfg.value_type(val);
                        let zero = b.ins().iconst(ty, 0);
                        let cond = b.ins().icmp(IntCC::Equal, val, zero);
                        Ok(Self::boolean(b, cond))
                    }
                    UnOp::Address | UnOp::Deref => unreachable!(),
                }
            }
            CheckedExprKind::Conditional(condition, yes, no) => {
                // Build separate blocks: evaluating both operands and using a
                // select would incorrectly execute the unchosen side effects.
                let test = self.condition(b, condition)?;
                let then_block = b.create_block();
                let else_block = b.create_block();
                let join = b.create_block();
                let ty = match &expr.ty {
                    CType::Void => types::I32, // no meaningful result value
                    CType::Struct { .. } | CType::Union { .. } => types::I64,
                    other => scalar_type(other)?,
                };
                let result = b.declare_var(ty);
                b.ins().brif(test, then_block, &[], else_block, &[]);

                b.switch_to_block(then_block);
                let left_value = self.expression(b, yes)?;
                b.def_var(result, left_value);
                b.ins().jump(join, &[]);

                b.switch_to_block(else_block);
                let right_value = self.expression(b, no)?;
                b.def_var(result, right_value);
                b.ins().jump(join, &[]);

                b.switch_to_block(join);
                Ok(b.use_var(result))
            }
            CheckedExprKind::Comma(left, right) => {
                // C comma expressions provide a sequence point. Preserve
                // effects from the left before computing the right value.
                self.expression(b, left)?;
                self.expression(b, right)
            }
            CheckedExprKind::Assign(lhs, rhs) => {
                let address = self.place(b, lhs)?;
                let rhs_value = self.expression(b, rhs)?;
                if matches!(&lhs.ty, CType::Struct { .. } | CType::Union { .. }) {
                    let size = lhs.ty.size_bytes(Target::default())
                        .ok_or("incomplete struct assignment")?;
                    Self::copy_bytes(b, address, rhs_value, size)?;
                    Ok(address)
                } else if let Some((shift, width, signed)) = Self::bit_field_details(lhs) {
                    Ok(Self::store_bit_field(b, address, rhs_value, shift, width, signed))
                } else {
                    b.ins().store(ir::MemFlagsData::new(), rhs_value, address, 0);
                    Ok(rhs_value)
                }
            }
            CheckedExprKind::Update { target, increment, postfix, computation_ty } => {
                // ++ and -- must evaluate the lvalue's address exactly once.
                // Postfix yields the loaded old value; prefix the stored new one.
                let address = self.place(b, target)?;
                let details = Self::bit_field_details(target);
                let old = if let Some((shift, width, signed)) = details {
                    Self::load_bit_field(b, address, shift, width, signed)
                } else {
                    b.ins().load(scalar_type(&target.ty)?,
                        ir::MemFlagsData::new(), address, 0)
                };
                let stored = if let CType::Pointer(element) = computation_ty {
                    let width = element.size_bytes(Target::default())
                        .ok_or("increment/decrement requires complete pointer element")?;
                    let width = i64::try_from(width)
                        .map_err(|_| "pointer element exceeds supported offset")?;
                    b.ins().iadd_imm_s(old, if *increment { width } else { -width })
                } else {
                    let promoted = Self::convert(b, old, &target.ty, computation_ty)?;
                    let one = b.ins().iconst(scalar_type(computation_ty)?, 1);
                    let computed = if *increment {
                        b.ins().iadd(promoted, one)
                    } else {
                        b.ins().isub(promoted, one)
                    };
                    Self::convert(b, computed, computation_ty, &target.ty)?
                };
                let stored = if let Some((shift, width, signed)) = details {
                    Self::store_bit_field(b, address, stored, shift, width, signed)
                } else {
                    b.ins().store(ir::MemFlagsData::new(), stored, address, 0);
                    stored
                };
                Ok(if *postfix { old } else { stored })
            }
            CheckedExprKind::CompoundAssign(op, lhs, rhs, computation_ty) => {
                // Never lower as lhs = lhs op rhs: a subscript, dereference,
                // or member expression may compute an address with side effects.
                let address = self.place(b, lhs)?;
                let details = Self::bit_field_details(lhs);
                let old = if let Some((shift, width, signed)) = details {
                    Self::load_bit_field(b, address, shift, width, signed)
                } else {
                    b.ins().load(scalar_type(&lhs.ty)?,
                        ir::MemFlagsData::new(), address, 0)
                };
                if let CType::Pointer(element) = computation_ty {
                    // C pointer +=/-= scales by the complete element size.
                    // The lvalue address above and right expression below
                    // are each computed exactly once.
                    let offset = self.integer_value(b, rhs)?;
                    let displacement = Self::pointer_displacement(
                        b, offset, &rhs.ty, element.as_ref())?;
                    let result = match op {
                        BinOp::Add => b.ins().iadd(old, displacement),
                        BinOp::Sub => b.ins().isub(old, displacement),
                        _ => return Err("invalid pointer compound operator".into()),
                    };
                    b.ins().store(ir::MemFlagsData::new(), result, address, 0);
                    return Ok(result);
                }
                let left = Self::convert(b, old, &lhs.ty, computation_ty)?;
                let mut right = self.integer_value(b, rhs)?;
                if matches!(op, BinOp::Shl | BinOp::Shr) &&
                    b.func.dfg.value_type(right) != b.func.dfg.value_type(left)
                {
                    right = Self::convert(b, right, &rhs.ty, computation_ty)?;
                }
                let unsigned = matches!(computation_ty, CType::Integer { unsigned: true, .. });
                let computed = match op {
                    BinOp::BitAnd => b.ins().band(left, right),
                    BinOp::BitOr => b.ins().bor(left, right),
                    BinOp::BitXor => b.ins().bxor(left, right),
                    BinOp::Add => b.ins().iadd(left, right),
                    BinOp::Sub => b.ins().isub(left, right),
                    BinOp::Mul => b.ins().imul(left, right),
                    BinOp::Div if unsigned => b.ins().udiv(left, right),
                    BinOp::Div => b.ins().sdiv(left, right),
                    BinOp::Mod if unsigned => b.ins().urem(left, right),
                    BinOp::Mod => b.ins().srem(left, right),
                    BinOp::Shl => b.ins().ishl(left, right),
                    BinOp::Shr if unsigned => b.ins().ushr(left, right),
                    BinOp::Shr => b.ins().sshr(left, right),
                    _ => return Err("unsupported compound assignment operator".into()),
                };
                let stored = Self::convert(b, computed, computation_ty, &lhs.ty)?;
                let stored = if let Some((shift, width, signed)) = details {
                    Self::store_bit_field(b, address, stored, shift, width, signed)
                } else {
                    b.ins().store(ir::MemFlagsData::new(), stored, address, 0);
                    stored
                };
                // Assignment expressions yield the value actually stored,
                // including narrowing back to char or another small type.
                Ok(stored)
            }
            CheckedExprKind::Binary(op, lhs, rhs) => {
                if matches!(op, BinOp::And | BinOp::Or) {
                    return self.short_circuit(b, *op, lhs, rhs);
                }
                if matches!(op, BinOp::Add | BinOp::Sub)
                    && matches!(&expr.ty, CType::Pointer(_))
                {
                    return self.pointer_offset(b, *op, lhs, rhs);
                }
                if *op == BinOp::Sub {
                    if let (CType::Pointer(element), CType::Pointer(_)) = (&lhs.ty, &rhs.ty) {
                        // The checker establishes compatible complete object
                        // pointees. Cranelift addresses are byte offsets;
                        // pointer difference is a signed element count.
                        let width = element.size_bytes(Target::default())
                            .ok_or("native ptrdiff requires a complete object type")?;
                        let width = i64::try_from(width)
                            .map_err(|_| "native ptrdiff element size exceeds i64")?;
                        let left = self.expression(b, lhs)?;
                        let right = self.expression(b, rhs)?;
                        let bytes = b.ins().isub(left, right);
                        return Ok(if width == 1 {
                            bytes
                        } else {
                            let scale = b.ins().iconst(types::I64, width);
                            b.ins().sdiv(bytes, scale)
                        });
                    }
                }
                // Checked pointer comparisons are represented using integer-valued
                // Cranelift addresses. The type checker has already validated the
                // operands and converted null pointer constants to pointer type.
                let pointer_comparison = matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
                    && (matches!(&lhs.ty, CType::Pointer(_)) || matches!(&rhs.ty, CType::Pointer(_)));
                let mut left = if pointer_comparison { self.expression(b, lhs)? } else { self.integer_value(b, lhs)? };
                let mut right = if pointer_comparison { self.expression(b, rhs)? } else { self.integer_value(b, rhs)? };
                if pointer_comparison {
                    // Null pointer constants may retain integer type in the checked IR.
                    // Compare pointer-sized addresses, never mismatched I32/I64 values.
                    if b.func.dfg.value_type(left) != types::I64 { left = Self::convert(b, left, &lhs.ty, &CType::Pointer(Box::new(CType::Void)))?; }
                    if b.func.dfg.value_type(right) != types::I64 { right = Self::convert(b, right, &rhs.ty, &CType::Pointer(Box::new(CType::Void)))?; }
                }
                let unsigned = pointer_comparison || matches!(&lhs.ty, CType::Integer { unsigned: true, .. });
                if matches!(op, BinOp::Shl | BinOp::Shr) {
                    // Cranelift's shift instructions operate at the width of
                    // the promoted left operand, independent of the promoted
                    // right operand's C type.
                    if b.func.dfg.value_type(right) != b.func.dfg.value_type(left) {
                        right = Self::convert(b, right, &rhs.ty, &lhs.ty)?;
                    }
                }
                let value = match op {
                    BinOp::Add => b.ins().iadd(left, right),
                    BinOp::Sub => b.ins().isub(left, right),
                    BinOp::Mul => b.ins().imul(left, right),
                    BinOp::BitAnd => b.ins().band(left, right),
                    BinOp::BitOr => b.ins().bor(left, right),
                    BinOp::BitXor => b.ins().bxor(left, right),
                    BinOp::Shl => b.ins().ishl(left, right),
                    BinOp::Shr if unsigned => b.ins().ushr(left, right),
                    BinOp::Shr => b.ins().sshr(left, right),
                    BinOp::Div => if unsigned { b.ins().udiv(left, right) } else { b.ins().sdiv(left, right) },
                    BinOp::Mod => if unsigned { b.ins().urem(left, right) } else { b.ins().srem(left, right) },
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        let cc = match op {
                            BinOp::Eq => IntCC::Equal,
                            BinOp::Ne => IntCC::NotEqual,
                            BinOp::Lt if unsigned => IntCC::UnsignedLessThan,
                            BinOp::Lt => IntCC::SignedLessThan,
                            BinOp::Le if unsigned => IntCC::UnsignedLessThanOrEqual,
                            BinOp::Le => IntCC::SignedLessThanOrEqual,
                            BinOp::Gt if unsigned => IntCC::UnsignedGreaterThan,
                            BinOp::Gt => IntCC::SignedGreaterThan,
                            BinOp::Ge if unsigned => IntCC::UnsignedGreaterThanOrEqual,
                            BinOp::Ge => IntCC::SignedGreaterThanOrEqual,
                            _ => unreachable!(),
                        };
                        let flag = b.ins().icmp(cc, left, right);
                        return Ok(Self::boolean(b, flag));
                    }
                    BinOp::And | BinOp::Or => unreachable!(),
                };
                Ok(value)
            }
            CheckedExprKind::Call(callee, args) => {
                // Cranelift 0.136 has fixed-arity SystemV signatures but no
                // variadic AL operand. Direct calls with only GP-class integer
                // and pointer arguments can use a tiny per-callsite tail thunk:
                // Cranelift places every actual argument normally, the thunk
                // zeroes EAX (AL=0 SSE arguments), then jumps to libc without
                // disturbing the caller's GP registers or stack arguments.
                let variadic = match &callee.ty {
                    CType::VariadicFunction { .. } => true,
                    CType::Pointer(inner) =>
                        matches!(inner.as_ref(), CType::VariadicFunction { .. }),
                    _ => false,
                };
                if variadic && !matches!(
                    (&callee.kind, &callee.ty),
                    (CheckedExprKind::Name(_), CType::VariadicFunction { .. })
                ) {
                    return Err("indirect SysV variadic calls are unsupported".into());
                }
                let mut values = Vec::with_capacity(args.len());
                for arg in args { values.push(self.expression(b, arg)?); }
                let inst = if let (CheckedExprKind::Name(name),
                    CType::VariadicFunction { result, .. }) = (&callee.kind, &callee.ty)
                {
                    let target = self.functions.get(name)
                        .map(|known| known.id)
                        .ok_or_else(|| format!("missing variadic function declaration for '{name}'"))?;
                    let mut sig = self.module.make_signature();
                    for arg in args {
                        if !matches!(arg.ty.unqualified(),
                            CType::Integer { .. } | CType::Pointer(_))
                        {
                            return Err(format!(
                                "SysV variadic call '{name}' requires GP-class arguments only; SSE and aggregate ABI unsupported"
                            ));
                        }
                        sig.params.push(AbiParam::new(scalar_type(&arg.ty)?));
                    }
                    if result.unqualified() != &CType::Void {
                        sig.returns.push(AbiParam::new(scalar_type(result)?));
                    }
                    let wrapper_name = format!("__mini_c_sysv_gpvar_{}", self.gp_vararg_thunks.len());
                    let wrapper = self.module.declare_function(
                        &wrapper_name, ClLinkage::Import, &sig
                    ).map_err(|e| format!("declaring SysV variadic thunk: {e}"))?;
                    self.gp_vararg_thunks.push(SysvGpVarargThunk { wrapper, target });
                    let reference = self.module.declare_func_in_func(wrapper, b.func);
                    b.ins().call(reference, &values)
                } else if let (CheckedExprKind::Name(name), CType::Function { .. }) =
                    (&callee.kind, &callee.ty)
                {
                    let id = self.functions.get(name)
                        .map(|known| known.id)
                        .ok_or_else(|| format!("missing function declaration for '{name}'"))?;
                    let refid = self.module.declare_func_in_func(id, b.func);
                    b.ins().call(refid, &values)
                } else {
                    let function_ty = match &callee.ty {
                        CType::Function { .. } => &callee.ty,
                        CType::Pointer(inner) if matches!(inner.as_ref(), CType::Function { .. }) =>
                            inner.as_ref(),
                        _ => return Err("indirect call requires a checked function pointer".into()),
                    };
                    let sig = signature(self.module, function_ty)?;
                    let sigref = b.func.import_signature(sig);
                    let address = self.expression(b, callee)?;
                    b.ins().call_indirect(sigref, address, &values)
                };
                let results = b.inst_results(inst);
                if let Some(&result) = results.first() {
                    Ok(result)
                } else {
                    // All valid uses of a void function are statement expressions.
                    Ok(b.ins().iconst(types::I32, 0))
                }
            }
        }
    }


    /// Each case/default is its own source-level entry point. Discover labels
    /// recursively across ordinary blocks and control statements, but never
    /// descend into a nested switch: that switch owns an independent case
    /// namespace (and will build its own dispatch map on lowering).
    fn collect_switch_cases<'a>(
        stmt: &'a CheckedStmt,
        labels: &mut Vec<(&'a CheckedStmt, Option<i128>)>,
    ) {
        match stmt {
            CheckedStmt::Case { value, body } => {
                labels.push((stmt, Some(*value)));
                Self::collect_switch_cases(body, labels);
            }
            CheckedStmt::Default(body) => {
                labels.push((stmt, None));
                Self::collect_switch_cases(body, labels);
            }
            CheckedStmt::Block(stmts) => {
                for stmt in stmts { Self::collect_switch_cases(stmt, labels); }
            }
            CheckedStmt::Label { body, .. } => Self::collect_switch_cases(body, labels),
            CheckedStmt::If { then, otherwise, .. } => {
                Self::collect_switch_cases(then, labels);
                if let Some(otherwise) = otherwise {
                    Self::collect_switch_cases(otherwise, labels);
                }
            }
            CheckedStmt::While { body, .. } | CheckedStmt::DoWhile { body, .. } =>
                Self::collect_switch_cases(body, labels),
            CheckedStmt::For(for_loop) => {
                if let Some(init) = &for_loop.init {
                    Self::collect_switch_cases(init, labels);
                }
                Self::collect_switch_cases(&for_loop.body, labels);
            }
            CheckedStmt::Switch { .. } => {} // inner switch owns its cases
            _ => {}
        }
    }

    /// Switch dispatch targets source case nodes, even when a case is nested
    /// under an if/loop/compound statement. Every node's address is stable for
    /// the duration of lowering this immutable checked function body. The
    /// pointer is used only as an identity key and is never dereferenced.
    fn checked_stmt_id(stmt: &CheckedStmt) -> usize {
        stmt as *const CheckedStmt as usize
    }

    /// Source labels are CFG entry blocks. Evaluate the controlling expression
    /// exactly once, then dispatch to the matching node (or default/exit).
    /// Ordinary switch-body entry is intentionally disconnected: C dispatch
    /// cannot execute statements preceding the first matching case.
    ///
    /// Local stack slots are bound independently of the runtime declaration
    /// path, so jumping over an initializer cannot inherit an invalid SSA
    /// stack_addr nor execute the skipped initializer.
    fn switch_statement(
        &mut self, b: &mut FunctionBuilder<'_>,
        control: &CheckedExpr, body: &CheckedStmt,
    ) -> Result<bool> {
        let control_value = self.expression(b, control)?;
        let control_type = scalar_type(&control.ty)?;
        let mut source_labels = Vec::new();
        Self::collect_switch_cases(body, &mut source_labels);
        if source_labels.is_empty() && !self.contains_referenced_label(body) {
            // The switch dispatch cannot enter a body without cases. A
            // function-scoped goto label inside it is a separate legal entry
            // and is handled below even if no case/default exists.
            return Ok(true);
        }

        let mut label_blocks = HashMap::new();
        let mut cases = Vec::new();
        let mut default = None;
        for (source, value) in source_labels {
            let block = b.create_block();
            label_blocks.insert(Self::checked_stmt_id(source), block);
            if let Some(value) = value {
                cases.push((value, block));
            } else {
                default = Some(block);
            }
        }

        let exit = b.create_block();
        for (value, arm) in cases {
            let next = b.create_block();
            // Checked case values were normalized to the promoted controlling
            // type. Preserve the same low-bit conversion for both signed and
            // unsigned long / int as the previous direct-arm emitter.
            let immediate = if control_type == types::I64 {
                value as i64
            } else {
                value as i32 as i64
            };
            let equal = b.ins().icmp_imm_s(IntCC::Equal, control_value, immediate);
            b.ins().brif(equal, arm, &[], next, &[]);
            b.switch_to_block(next);
        }
        b.ins().jump(default.unwrap_or(exit), &[]);

        self.breaks.push((exit, false));
        self.switch_labels.push(label_blocks);
        // The body is emitted for its nested case entry blocks, not as a
        // fallthrough successor of the switch dispatch or any outer block.
        let disconnected_entry = b.create_block();
        b.switch_to_block(disconnected_entry);
        let live = self.statement(b, body)?;
        if live { b.ins().jump(exit, &[]); }
        self.switch_labels.pop();
        let (_, break_reaches_exit) = self.breaks.pop().expect("active switch break scope");
        b.switch_to_block(exit);
        if default.is_some() && !live && !break_reaches_exit {
            // All dispatch paths end in returns/traps, with no path to the
            // synthetic exit. Retain the existing conservative fallthrough
            // contract for a non-void function with no trailing return.
            b.ins().trap(ir::TrapCode::unwrap_user(1));
            Ok(false)
        } else {
            Ok(true)
        }
    }

    /// Is a case/default targeted by the innermost active switch dispatch in
    /// this statement subtree? Ignore case labels owned by a nested switch.
    fn contains_active_switch_case(&self, stmt: &CheckedStmt) -> bool {
        let Some(mapping) = self.switch_labels.last() else { return false };
        fn walk(stmt: &CheckedStmt, mapping: &HashMap<usize, ir::Block>) -> bool {
            match stmt {
                CheckedStmt::Case { body, .. } | CheckedStmt::Default(body) =>
                    mapping.contains_key(&Lower::checked_stmt_id(stmt)) || walk(body, mapping),
                CheckedStmt::Block(stmts) => stmts.iter().any(|stmt| walk(stmt, mapping)),
                CheckedStmt::Label { body, .. } => walk(body, mapping),
                CheckedStmt::If { then, otherwise, .. } =>
                    walk(then, mapping) ||
                        otherwise.as_ref().is_some_and(|s| walk(s, mapping)),
                CheckedStmt::While { body, .. } | CheckedStmt::DoWhile { body, .. } =>
                    walk(body, mapping),
                CheckedStmt::For(for_loop) =>
                    for_loop.init.as_ref().is_some_and(|s| walk(s, mapping))
                    || walk(&for_loop.body, mapping),
                CheckedStmt::Switch { .. } => false,
                _ => false,
            }
        }
        walk(stmt, mapping)
    }

    /// Does this subtree have an ordinary C label targeted by a goto?
    /// A goto can enter a switch without evaluating its control expression;
    /// case/default dispatch remains independent and scoped per switch.
    fn contains_referenced_label(&self, stmt: &CheckedStmt) -> bool {
        match stmt {
            CheckedStmt::Label { name, body } =>
                self.referenced_labels.contains(name) || self.contains_referenced_label(body),
            CheckedStmt::Block(stmts) => stmts.iter()
                .any(|stmt| self.contains_referenced_label(stmt)),
            CheckedStmt::If { then, otherwise, .. } =>
                self.contains_referenced_label(then)
                    || otherwise.as_ref().is_some_and(|s| self.contains_referenced_label(s)),
            CheckedStmt::While { body, .. } | CheckedStmt::DoWhile { body, .. }
            | CheckedStmt::Switch { body, .. } | CheckedStmt::Case { body, .. }
            | CheckedStmt::Default(body) => self.contains_referenced_label(body),
            CheckedStmt::For(for_loop) =>
                for_loop.init.as_ref().is_some_and(|s| self.contains_referenced_label(s))
                    || self.contains_referenced_label(&for_loop.body),
            _ => false,
        }
    }

    /// Lower a lexical block even if it has no natural incoming fallthrough:
    /// a goto may enter a label inside nested compounds or control constructs.
    /// Dead declarations still register storage, but never execute initializers
    /// or default stores. Unreachable control entries are kept disconnected
    /// from normal execution while their nested label blocks remain reachable.
    fn lower_block(
        &mut self, b: &mut FunctionBuilder<'_>,
        stmts: &[CheckedStmt], mut live: bool,
    ) -> Result<bool> {
        self.scopes.push(HashMap::new());
        for stmt in stmts {
            let mut tail = stmt;
            let mut chain = Vec::new();
            while let CheckedStmt::Label { name, body } = tail {
                chain.push(name.as_str());
                tail = body;
            }
            let referenced = chain.iter()
                .any(|name| self.referenced_labels.contains(*name));
            if !live && !referenced {
                match tail {
                    CheckedStmt::Decl(CheckedDecl { name, ty, storage, .. }) => {
                        if !matches!(*storage, StorageClass::Default | StorageClass::Register) {
                            return Err(format!("M2.5 does not emit storage-class local '{name}'"));
                        }
                        self.bind_storage(b, name, ty)?;
                    }
                    CheckedStmt::Block(inner) if self.contains_referenced_label(tail)
                        || self.contains_active_switch_case(tail) =>
                    {
                        live = self.lower_block(b, inner, false)?;
                    }
                    CheckedStmt::If { .. } | CheckedStmt::While { .. }
                    | CheckedStmt::DoWhile { .. } | CheckedStmt::For(_)
                    | CheckedStmt::Switch { .. }
                        if self.contains_referenced_label(tail)
                            || self.contains_active_switch_case(tail) =>
                    {
                        // The lexical entry is bypassed. Build its normal
                        // condition/init/switch-dispatch paths on a
                        // disconnected block while the nested goto or case
                        // entry receives its own real CFG edge.
                        let dead_entry = b.create_block();
                        b.switch_to_block(dead_entry);
                        live = self.statement(b, tail)?;
                    }
                    CheckedStmt::Case { .. } | CheckedStmt::Default(_)
                        if self.contains_active_switch_case(tail)
                            || self.contains_referenced_label(tail) =>
                    {
                        let dead_entry = b.create_block();
                        b.switch_to_block(dead_entry);
                        live = self.statement(b, tail)?;
                    }
                    _ => {}
                }
                continue;
            }
            // Each consecutive label owns a distinct CFG block; fallthrough
            // to it is explicit, and a goto can directly target any label.
            for name in chain {
                let target = *self.labels.get(name)
                    .ok_or_else(|| format!("unplanned native label '{name}'"))?;
                if live {
                    b.ins().jump(target, &[]);
                }
                b.switch_to_block(target);
                live = true;
            }
            live = self.statement(b, tail)?;
        }
        self.scopes.pop();
        Ok(live)
    }

    /// Returns whether control can fall through to the next statement.
    fn statement(&mut self, b: &mut FunctionBuilder<'_>, stmt: &CheckedStmt) -> Result<bool> {
        match stmt {
            CheckedStmt::Block(stmts) => self.lower_block(b, stmts, true),
            CheckedStmt::Typedef { .. } | CheckedStmt::Tag => Ok(true),
            CheckedStmt::Goto { target } => {
                let block = *self.labels.get(target)
                    .ok_or_else(|| format!("unplanned native goto target '{target}'"))?;
                b.ins().jump(block, &[]);
                Ok(false)
            }
            CheckedStmt::Label { name, body } => {
                // Direct label child of if/while/do/for. Compound-statement
                // label chains are handled by lower_block, which can resume
                // after a preceding terminator.
                let target = *self.labels.get(name)
                    .ok_or_else(|| format!("unplanned native label '{name}'"))?;
                b.ins().jump(target, &[]);
                b.switch_to_block(target);
                self.statement(b, body)
            },
            CheckedStmt::Decl(CheckedDecl { name, ty, storage, initializer, .. }) => {
                if !matches!(*storage, StorageClass::Default | StorageClass::Register) {
                    return Err(format!("M2.5 does not emit storage-class local '{name}'"));
                }
                self.bind(b, name, ty, None)?;
                if let Some(initializer) = initializer {
                    let address = self.object_address(b, name)?;
                    self.initialize_object(b, address, ty, initializer)?;
                }
                Ok(true)
            }
            CheckedStmt::Expr(expr) => {
                if let Some(expr) = expr { self.expression(b, expr)?; }
                Ok(true)
            }
            CheckedStmt::Return(value) => {
                if let Some(expr) = value {
                    let v = self.expression(b, expr)?;
                    b.ins().return_(&[v]);
                } else {
                    b.ins().return_(&[]);
                }
                Ok(false)
            }
            CheckedStmt::If { cond, then, otherwise } => {
                let cond = self.condition(b, cond)?;
                let then_block = b.create_block();
                let join = b.create_block();
                let else_block = if otherwise.is_some() { b.create_block() } else { join };
                b.ins().brif(cond, then_block, &[], else_block, &[]);
                b.switch_to_block(then_block);
                let then_live = self.statement(b, then)?;
                if then_live { b.ins().jump(join, &[]); }
                let else_live = if let Some(otherwise) = otherwise {
                    b.switch_to_block(else_block);
                    let live = self.statement(b, otherwise)?;
                    if live { b.ins().jump(join, &[]); }
                    live
                } else { true };
                if then_live || else_live {
                    b.switch_to_block(join);
                    Ok(true)
                } else {
                    // Every predecessor has returned. Seal an intentionally
                    // unreachable join so Cranelift still sees a terminator.
                    b.switch_to_block(join);
                    b.ins().trap(ir::TrapCode::unwrap_user(1));
                    Ok(false)
                }
            }
            CheckedStmt::While { cond, body } => {
                let header = b.create_block();
                let loop_body = b.create_block();
                let exit = b.create_block();
                b.ins().jump(header, &[]);
                b.switch_to_block(header);
                let cond = self.condition(b, cond)?;
                b.ins().brif(cond, loop_body, &[], exit, &[]);
                self.loops.push((exit, header));
                self.breaks.push((exit, false));
                b.switch_to_block(loop_body);
                let body_live = self.statement(b, body)?;
                self.breaks.pop();
                self.loops.pop();
                if body_live { b.ins().jump(header, &[]); }
                b.switch_to_block(exit);
                Ok(true)
            }
            CheckedStmt::DoWhile { body, cond } => {
                let loop_body = b.create_block();
                let test = b.create_block();
                let exit = b.create_block();
                // Unlike while, do-while always executes the body first.
                b.ins().jump(loop_body, &[]);
                self.loops.push((exit, test));
                self.breaks.push((exit, false));
                b.switch_to_block(loop_body);
                let body_live = self.statement(b, body)?;
                self.breaks.pop();
                self.loops.pop();
                if body_live { b.ins().jump(test, &[]); }
                // A continue in a do-while must evaluate its condition,
                // not restart the body directly.
                b.switch_to_block(test);
                let value = self.condition(b, cond)?;
                b.ins().brif(value, loop_body, &[], exit, &[]);
                b.switch_to_block(exit);
                Ok(true)
            }
            CheckedStmt::For(for_loop) => {
                let (init, cond, step, body) = (
                    &for_loop.init, &for_loop.cond, &for_loop.step, &for_loop.body,
                );
                self.scopes.push(HashMap::new());
                if let Some(init) = init {
                    if !self.statement(b, init)? {
                        return Err("invalid terminating for-loop initializer".into());
                    }
                }
                let header = b.create_block();
                let loop_body = b.create_block();
                // Unlike while, continue must evaluate the for increment.
                let increment = b.create_block();
                let exit = b.create_block();
                b.ins().jump(header, &[]);
                b.switch_to_block(header);
                if let Some(cond) = cond {
                    let value = self.condition(b, cond)?;
                    b.ins().brif(value, loop_body, &[], exit, &[]);
                } else {
                    b.ins().jump(loop_body, &[]);
                }
                self.loops.push((exit, increment));
                self.breaks.push((exit, false));
                b.switch_to_block(loop_body);
                let body_live = self.statement(b, body)?;
                self.breaks.pop();
                self.loops.pop();
                if body_live { b.ins().jump(increment, &[]); }
                b.switch_to_block(increment);
                if let Some(step) = step {
                    self.expression(b, step)?;
                }
                b.ins().jump(header, &[]);
                b.switch_to_block(exit);
                self.scopes.pop();
                Ok(true)
            }
            CheckedStmt::Switch { control, body } => self.switch_statement(b, control, body),
            CheckedStmt::Case { body, .. } | CheckedStmt::Default(body) => {
                let target = self.switch_labels.last()
                    .and_then(|scope| scope.get(&Lower::checked_stmt_id(stmt)))
                    .copied()
                    .ok_or("native case/default label has no switch dispatch block")?;
                b.ins().jump(target, &[]);
                b.switch_to_block(target);
                self.statement(b, body)
            },
            CheckedStmt::Break => {
                let (break_dest, reaches_exit) = self.breaks.last_mut()
                    .ok_or("break outside loop or switch in checked IR")?;
                *reaches_exit = true;
                b.ins().jump(*break_dest, &[]);
                Ok(false)
            }
            CheckedStmt::Continue => {
                let (_, continue_dest) = self.loops.last()
                    .copied().ok_or("continue outside loop in checked IR")?;
                b.ins().jump(continue_dest, &[]);
                Ok(false)
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_goto_compiles_direct_forward_backward_and_label_chains() {
        for source in [
            "int f(int x) { if(x) goto done; x=3; done: return x; }",
            "int f(int x) { again: x=x-1; if(x>0) goto again; return x; }",
            "int f(void) { goto second; first: return 1; second: third: return 2; }",
            "int f(void) { int x=3; if(x) goto done; x=7; done: return x; }",
            "int f(void) { return 1; after: return 2; }",
        ] {
            compile_source(source).unwrap_or_else(|why| panic!("{why}: {source}"));
        }
    }

    #[test]
    fn native_goto_supports_skipped_local_storage_and_nested_compound_labels() {
        for source in [
            "int f(void) { goto done; int y=3; done: y=7; return y; }",
            "int f(void) { goto done; int a[3]; done: a[2]=7; return a[2]; }",
            "int f(void) { goto inner; { int y=3; inner: y=7; return y; } }",
            "int f(void) { goto inner; { int x=4; { inner: x=2; } } return 0; }",
        ] {
            compile_source(source).unwrap_or_else(|why| panic!("{why}: {source}"));
        }
    }

    #[test]
    fn native_goto_supports_nested_if_and_loop_labels() {
        for source in [
            "int f(int x) { goto done; if(x) { done: return 2; } return 0; }",
            "int f(int x) { if(x) here: return 3; goto here; }",
            "int f(void) { goto here; while(0) { here: return 3; } return 0; }",
            "int f(void) { goto here; do { here: return 4; } while(0); return 0; }",
            "int f(void) { goto here; for(int i=0;i<2;i++) { here: i=1; break; } return 5; }",
            "int f(void) { if(1) goto here; while(0) { here: break; } return 3; }",
        ] {
            compile_source(source).unwrap_or_else(|why| panic!("{why}: {source}"));
        }
    }

    #[test]
    fn native_goto_supports_switch_arm_labels_and_no_case_switch() {
        for source in [
            "int f(int x) { goto done; switch(x) {case 1: done: return 2; default: return 0;} }",
            "int f(int x) { goto done; switch(x) {done: return 2;} return 0; }",
            "int f(int x) { goto done; switch(x) {case 1: while(x) {done: return 2;} break;} return 0; }",
            "int f(int x) { switch(x) {case 1: goto done; case 2: done: return 2;} return 0; }",
        ] {
            compile_source(source).unwrap_or_else(|why| panic!("{why}: {source}"));
        }
    }

    #[test]
    fn native_goto_dead_control_exits_do_not_require_fallback_return() {
        for source in [
            "int f(void) { goto inside; if(0) { inside: return 11; } }",
            "int f(void) { goto inside; while(0) { inside: return 12; } }",
            "int f(void) { goto inside; do { inside: return 13; } while(0); }",
            "int f(void) { goto inside; for(int i=0;i<3;i++) { inside: return 14; } }",
            "int f(void) { goto inside; switch(0) { case 0: inside: return 15; } }",
            "int f(void) { goto inside; switch(0) { inside: return 16; } }",
        ] {
            compile_source(source).unwrap_or_else(|why| panic!("{why}: {source}"));
        }
    }

    #[test]
    fn native_reachable_end_still_rejects_missing_return() {
        for source in [
            "int f(int x) { if(x) return 1; }",
            "int f(int x) { if(x) goto inside; return 1; while(x) { inside: if(x==2) return 2; break; } }",
        ] {
            let error = compile_source(source).unwrap_err();
            assert!(error.contains("may reach its end without a return"), "{error}: {source}");
        }
    }

    #[test]
    fn sysv_gp_only_variadic_printf_emits_zero_al_tail_thunk() {
        use object::{Object, ObjectSymbol};
        let source = concat!(
            "extern int printf(const char *fmt, ...); ",
            "int main(void) { printf(\"Hello, %s!\\n\", \"world\"); return 0; }"
        );
        let bytes = compile_source(source).expect("GP-only SysV printf call");
        let elf = object::File::parse(bytes.as_slice()).expect("ELF");
        let names = elf.symbols().filter_map(|symbol| symbol.name().ok())
            .collect::<Vec<_>>();
        assert!(names.contains(&"printf"), "must retain real libc relocation target");
        assert!(names.iter().any(|name| name.starts_with("__mini_c_sysv_gpvar_")),
            "missing local varargs thunk symbol");
        assert!(bytes.windows(3).any(|w| w == [0x31, 0xc0, 0xe9]),
            "expected xor eax,eax; jmp rel32 ABI tail thunk");
    }

    #[test]
    fn sysv_gp_variadic_callers_can_pass_stack_arguments() {
        let source = concat!(
            "extern int printf(const char *fmt, ...); ",
            "int main(void) { ",
            "printf(\"%s %d %d %d %d %d %d\\n\", \"x\", 1, 2, 3, 4, 5, 6); ",
            "return 0; }"
        );
        assert!(!compile_source(source).unwrap().is_empty());
    }

    #[test]
    fn indirect_sysv_variadic_call_fails_closed() {
        let source = concat!(
            "extern int printf(const char *, ...); ",
            "int main(void) { int (*p)(const char *, ...) = printf; ",
            "return p(\"%s\", \"word\"); }"
        );
        let err = compile_source(source).unwrap_err();
        // Until function-designator decay is supported in all initializer
        // contexts, the front-end may reject the function-pointer assignment.
        assert!(err.contains("indirect SysV variadic")
                || err.contains("unresolved variable"), "{err}");
    }

    #[test]
    fn glibc_late_asm_label_precedes_first_elf_import() {
        use object::{Object, ObjectSection, ObjectSymbol};
        let source = concat!(
            "extern int localput(const char *); ",
            "extern int localput(const char *) __asm__(\"puts\"); ",
            "int main(void) { return localput(\"hello\") < 0; }"
        );
        let obj = compile_source(source).expect("compatible late asm label");
        let elf = object::File::parse(obj.as_slice()).expect("valid ELF");
        let names = elf.symbols().filter_map(|symbol| symbol.name().ok())
            .collect::<Vec<_>>();
        assert!(names.contains(&"puts"), "expected actual extern link name");
        assert!(!names.contains(&"localput"), "source name is not an ELF symbol");
        assert!(elf.sections().flat_map(|s| s.relocations()).any(|(_, relocation)|
            match relocation.target() {
                object::RelocationTarget::Symbol(index) =>
                    elf.symbol_by_index(index).ok()
                        .and_then(|symbol| symbol.name().ok()) == Some("puts"),
                _ => false,
            }
        ), "direct call must relocate to renamed imported symbol");
    }

    #[test]
    fn glibc_late_asm_label_before_definition_is_used() {
        // The import is registered only once all prototypes preceding the
        // function body have been seen, not at the first unlabelled prototype.
        let source = concat!(
            "extern int alias(const char *); ",
            "extern int alias(const char *) __asm__(\"puts\"); ",
            "int main(void) { alias(\"ok\"); return 0; }"
        );
        assert!(!compile_source(source).unwrap().is_empty());
    }

    #[test]
    fn variadic_calls_fail_with_explicit_sysv_abi_diagnostic() {
        // GP-class direct variadic calls are supported. Indirect calls must
        // still fail explicitly rather than lowering with a wrong ABI.
        assert!(compile_source(
            "extern int printf(const char *, ...); int main(void) { return printf(\"%s\", \"hi\"); }"
        ).is_ok());
        let error = compile_source(
            "int main(void) { int (*callback)(const char *, ...) = 0; return callback(\"%s\", \"hi\"); }"
        ).unwrap_err();
        assert!(error.contains("indirect SysV variadic"), "{error}");
        assert!(compile_source(
            "extern int puts(const char *); int main(void) { return puts(\"hello\"); }"
        ).is_ok());
    }

    #[test]
    fn external_assembler_alias_is_the_actual_elf_symbol() {
        use object::{Object, ObjectSection, ObjectSymbol};
        let source = concat!(
            "extern int localput(const char *) __asm__(\"\" \"puts\"); ",
            "int main(void) { localput(\"hello from alias\"); return 0; }"
        );
        let obj = compile_source(source).expect("compile source-level alias");
        let elf = object::File::parse(obj.as_slice()).expect("valid ELF object");
        let symbols = elf.symbols().filter_map(|s| s.name().ok().map(str::to_owned))
            .collect::<Vec<_>>();
        assert!(symbols.iter().any(|s| s == "puts"), "{symbols:?}");
        assert!(!symbols.iter().any(|s| s == "localput"), "{symbols:?}");
        assert!(elf.sections().flat_map(|section| section.relocations())
            .any(|(_, relocation)| match relocation.target() {
                object::RelocationTarget::Symbol(index) =>
                    elf.symbol_by_index(index).ok()
                        .and_then(|symbol| symbol.name().ok()) == Some("puts"),
                _ => false,
            }), "expected call relocation targeting true external symbol 'puts'");
    }

    #[test]
    fn later_glibc_asm_link_name_rebinds_unreferenced_prototype() {
        use object::{Object, ObjectSection, ObjectSymbol};
        let src = concat!(
            "extern int localput(const char *); ",
            "extern int localput(const char *) __asm__(\"puts\"); ",
            "int main(void) { return localput(\"alias override\"); }"
        );
        let bytes = compile_source(src).expect("late asm label should retarget unused prototype");
        let elf = object::File::parse(bytes.as_slice()).expect("valid ELF");
        assert!(elf.sections().flat_map(|section| section.relocations())
            .any(|(_, relocation)| match relocation.target() {
                object::RelocationTarget::Symbol(index) =>
                    elf.symbol_by_index(index).ok()
                        .and_then(|symbol| symbol.name().ok()) == Some("puts"),
                _ => false,
            }), "call relocation must use the later assembler alias");

        // The real glibc scanf prototypes use a variadic asm alias.
        let src = concat!(
            "extern int fscanf(void *, const char *, ...); ",
            "extern int fscanf(void *, const char *, ...) __asm__(\"__isoc23_fscanf\"); ",
            "int main(void) { return 0; }"
        );
        assert!(compile_source(src).is_ok());
    }

    #[test]
    fn late_asm_alias_rebinds_deferred_function_pointer_relocations() {
        use object::{Object, ObjectSection, ObjectSymbol};
        let src = concat!(
            "extern int localput(const char *); ",
            "int (*callback)(const char *) = localput; ",
            "extern int localput(const char *) __asm__(\"puts\"); ",
            "int main(void) { return callback(\"hello\"); }"
        );
        let bytes = compile_source(src).expect("late alias in global pointer initializer");
        let elf = object::File::parse(bytes.as_slice()).expect("valid ELF");
        assert!(elf.sections().flat_map(|section| section.relocations())
            .any(|(_, relocation)| match relocation.target() {
                object::RelocationTarget::Symbol(index) =>
                    elf.symbol_by_index(index).ok()
                        .and_then(|symbol| symbol.name().ok()) == Some("puts"),
                _ => false,
            }), "deferred pointer relocation must use the actual ELF alias");
    }

    #[test]
    fn late_asm_alias_after_native_body_is_rejected() {
        let src = concat!(
            "extern int f(void); ",
            "int use(void) { return f(); } ",
            "extern int f(void) __asm__(\"other\");"
        );
        let error = compile_source(src).unwrap_err();
        assert!(error.contains("conflicting assembler linkage name"), "{error}");
    }

    #[test]
    fn external_assembler_alias_keeps_function_pointer_relocations() {
        use object::{Object, ObjectSection, ObjectSymbol};
        let source = concat!(
            "extern int c_library_name(const char *) __asm__(\"puts\"); ",
            "int (*callback)(const char *) = c_library_name; ",
            "int main(void) { return callback(\"alias\") < 0; }"
        );
        let obj = compile_source(source).expect("compile alias address relocation");
        let elf = object::File::parse(obj.as_slice()).expect("valid ELF object");
        assert!(elf.sections().flat_map(|section| section.relocations())
            .any(|(_, relocation)| match relocation.target() {
                object::RelocationTarget::Symbol(index) =>
                    elf.symbol_by_index(index).ok()
                        .and_then(|symbol| symbol.name().ok()) == Some("puts"),
                _ => false,
            }), "function pointer initializer should use the alias link-name");
    }

    #[test]
    fn unused_hosted_prototypes_do_not_require_unimplemented_native_abis() {
        let src = concat!(
            "struct Cookie { long first; long second; }; ",
            "extern struct Cookie cookie_open(struct Cookie); ",
            "extern int cookie_consume(struct Cookie); ",
            "extern double hosted_math(double); ",
            "extern _Float128 hosted_quad(_Float128); ",
            "int main(void) { return 0; }"
        );
        let object = compile_source(src).expect("unused hosted prototypes are type-checked only");
        assert_eq!(&object[..4], b"\x7fELF");

        // The supported subset of unrelated external calls still emits
        // real relocations and must not be skipped.
        assert!(compile_source(
            "extern int puts(const char *); int main(void) { return puts(\"hi\"); }"
        ).is_ok());
    }

    #[test]
    fn used_hosted_prototype_does_not_get_an_incorrect_native_signature() {
        let src = concat!(
            "struct Cookie { long first; long second; }; ",
            "extern struct Cookie cookie_open(void); ",
            "int main(void) { cookie_open(); return 0; }"
        );
        let error = compile_source(src).unwrap_err();
        assert!(error.contains("missing function declaration"), "{error}");

        let src = concat!(
            "struct Cookie { long first; long second; }; ",
            "extern int cookie_consume(struct Cookie); ",
            "int main(void) { return &cookie_consume != 0; }"
        );
        assert!(compile_source(src).is_err(), "function address must not become an invalid relocation");

        // Declarations must still agree semantically even if Cranelift
        // cannot materialize their signatures.
        assert!(compile_source(
            "extern double hosted(double); extern double hosted(int); int main(void) { return 0; }"
        ).is_err());
    }

    #[test]
    fn compiles_simple_arithmetic_to_native_object() {
        let obj = compile_source("int main(void) { int x = 2; x = x * 3 + 1; return x; }").unwrap();
        assert!(obj.len() > 128, "expected nontrivial ELF object");
        assert_eq!(&obj[..4], b"\x7fELF");
    }

    #[test]
    fn compiles_functions_calls_recursive_control_flow() {
        let src = "int fact(int n) { if (n <= 1) return 1; return n * fact(n - 1); }\
                   int main(void) { return fact(5); }";
        assert!(!compile_source(src).unwrap().is_empty());
    }

    #[test]
    fn compiles_nested_loops_break_continue_and_shadowing() {
        let src = "int f(int limit) { int x = 0; while (x < limit) { \
                   { int x = 3; if (x == 0) break; } \
                   x = x + 1; if (x == 2) continue; if (x == 8) break; \
                   } return x; } int main(void) { return f(10); }";
        assert!(!compile_source(src).unwrap().is_empty());
    }

    #[test]
    fn emits_qualified_aggregate_initializers_and_copy() {
        let sources = [
            "struct Pair { int x; int y; }; const struct Pair global = {3,4}; int main(void) { return global.x + global.y - 7; }",
            "struct Pair { int x; int y; }; int main(void) { struct Pair src={3,4}; const struct Pair dst=src; return dst.x + dst.y - 7; }",
            "struct S { int matrix[2][2]; }; const struct S global = {{{1,2},{3,4}}}; int main(void) { return global.matrix[1][1] - 4; }",
            "struct S { int matrix[2][2]; }; int main(void) { const struct S local = {{{1,2},{3,4}}}; return local.matrix[1][1] - 4; }",
            "const int numbers[3] = {4,5,6}; int main(void) { return numbers[1] - 5; }",
        ];
        for source in sources {
            let object = compile_source(source).unwrap_or_else(|e|
                panic!("qualified aggregate lowering failed: {source}: {e}"));
            assert_eq!(&object[..4], b"\x7fELF");
        }
    }

    #[test]
    fn emits_const_character_array_initializers() {
        let sources = [
            "const char global[] = \"abc\"; int main(void) { return global[1] - 98; }",
            "int main(void) { const char local[3] = \"hi\"; return local[0] - 104; }",
            "struct Record { const char text[5]; }; struct Record global = {\"test\"}; int main(void) { return global.text[1] - 101; }",
            "struct Record { const char text[5]; }; int main(void) { struct Record local = {\"test\"}; return local.text[1] - 101; }",
        ];
        for source in sources {
            let object = compile_source(source).unwrap_or_else(|e|
                panic!("qualified character array lowering failed: {source}: {e}"));
            assert_eq!(&object[..4], b"\x7fELF");
        }
    }

    #[test]
    fn emits_short_local_global_and_parameter_types() {
        let sources = [
            "short global = 7; int main(void) { short local = global; return local - 7; }",
            "int add(short x, unsigned short y) { return x + y; } int main(void) { return add(-3, 9) - 6; }",
            "int main(void) { short x = -3; unsigned short y = 4; x += y; return x - 1; }",
        ];
        for source in sources {
            let object = compile_source(source).unwrap_or_else(|e|
                panic!("short codegen failed: {source}: {e}"));
            assert_eq!(&object[..4], b"\x7fELF");
        }
    }

    #[test]
    fn handles_unsigned_and_long_conversions() {
        let src = "unsigned long a(unsigned long x, unsigned int y) { \
                   return x + y * 2; } int main(void) { return a(50,3) == 56; }";
        assert!(!compile_source(src).unwrap().is_empty());
    }

    #[test]
    fn backend_supports_global_objects_and_rejects_incomplete_arrays() {
        assert!(!compile_source("int global = 3; int main(void) { return global; }").unwrap().is_empty());
        let error = compile_source("int main(void) { int a[]; return 0; }").unwrap_err();
        assert!(error.contains("incomplete"), "{error}");
    }

    #[test]
    fn compile_array_and_pointer_memory_operations() {
        let src = "int f(int *p, int n) { p[n] = 13; return *(p+n); } \
                   int main(void) { int a[3]; a[0]=2; a[1]=3; return f(a,2); }";
        let obj = compile_source(src).unwrap();
        assert_eq!(&obj[..4], b"\x7fELF");
        let src = "int main(void) { int a[2][3]; a[1][2]=25; return a[1][2]; }";
        assert!(!compile_source(src).unwrap().is_empty());
    }

    #[test]
    fn rejects_fallthrough_in_non_main_nonvoid() {
        let error = compile_source("int f(int x) { if (x) return x; }").unwrap_err();
        assert!(error.contains("may reach its end"), "{error}");
        assert!(!compile_source("int main(void) { int x = 1; }").unwrap().is_empty());
    }

    #[test]
    fn implicit_function_designators_initialize_static_and_local_pointers() {
        let src = "int identity(int x) { return x; } \
                   int (*global)(int) = identity; \
                   struct WithCallback { int (*fn)(int); }; \
                   struct WithCallback callbacks[1] = {{identity}}; \
                   int main(void) { int (*local)(int) = identity; \
                       return local != 0 && global != 0; }";
        let obj = compile_source(src).unwrap();
        assert_eq!(&obj[..4], b"\x7fELF");
    }

    #[test]
    fn emits_function_address_relocations_for_static_objects() {
        let source = "int identity(int x); \
                      int (*callback)(int) = &identity; \
                      struct Callback { int pad; int (*fn)(int); }; \
                      struct Callback holder = {7, &identity}; \
                      int (*table[2])(int) = {&identity, 0}; \
                      int identity(int x) { return x; }";
        let obj = compile_source(source).unwrap();
        assert_eq!(&obj[..4], b"\x7fELF");
        use object::{Object, ObjectSection, ObjectSymbol};
        let file = object::File::parse(obj.as_slice()).expect("valid ELF");
        let identity_refs = file.sections().flat_map(|section| section.relocations())
            .filter(|(_, relocation)| match relocation.target() {
                object::RelocationTarget::Symbol(index) => file.symbol_by_index(index)
                    .ok().and_then(|symbol| symbol.name().ok()) == Some("identity"),
                _ => false,
            }).count();
        assert_eq!(identity_refs, 3, "expected scalar, struct and array function relocations");
    }

    #[test]
    fn compiles_local_global_and_member_function_pointer_calls() {
        let examples = [
            "int twice(int v) { return v*2; } \
             int main(void) { int (*fp)(int) = twice; return fp(21)-42; }",
            "int plus(int v) { return v+1; } \
             int (*global)(int) = plus; int main(void) { return global(41)-42; }",
            "int plus(int v) { return v+1; } \
             struct Hooks { int (*call)(int); }; \
             struct Hooks hooks = {plus}; \
             int main(void) { return hooks.call(41)-42; }",
            "int plus(int v) { return v+1; } \
             int main(void) { return (*plus)(41)-42; }",
            "void side_effect(int *p) { *p = 42; } \
             int main(void) { int v=0; void (*fp)(int *) = side_effect; \
                 fp(&v); return v-42; }",
        ];
        for source in examples {
            let obj = compile_source(source).unwrap();
            assert_eq!(&obj[..4], b"\x7fELF");
        }
    }

    #[test]
    fn accepts_prototypes_before_definition_and_externals() {
        let src = "int add(int, int); int main(void) { return add(3,4); }\
                   int add(int x, int y) { return x + y; }";
        assert!(!compile_source(src).unwrap().is_empty());
    }
    #[test]
    fn accepts_completed_tag_in_redeclared_function_signature() {
        let src = "struct Node; int read(struct Node *p); \
                   struct Node { int value; }; \
                   int read(struct Node *p) { return p->value; }";
        assert!(!compile_source(src).unwrap().is_empty());
    }

    #[test]
    fn file_scope_tentative_struct_type_completes_without_object_redeclaration() {
        let source = "struct Node; struct Node global; \
                      struct Node { int value; }; \
                      int main(void) { global.value = 7; return global.value - 7; }";
        let obj = compile_source(source).unwrap();
        assert_eq!(&obj[..4], b"\x7fELF");
        let incomplete = compile_source("struct Missing; struct Missing global;");
        assert!(incomplete.unwrap_err().contains("incomplete global object"));
        let external = compile_source("extern struct Missing external;");
        assert!(external.is_ok(), "{external:?}");
    }

    #[test]
    fn tentative_unknown_bound_global_array_defaults_to_one_element() {
        let src = "int a[]; int main(void) { a[0] = 7; return a[0] - 7; }";
        let obj = compile_source(src).unwrap();
        assert_eq!(&obj[..4], b"\x7fELF");
        assert!(!compile_source("extern int a[];").unwrap().is_empty());
    }

    #[test]
    fn compiles_bitwise_integer_expressions() {
        let src = "int f(unsigned int x) { return (~x & 255) | (x ^ 23); }\
                   int g(int a, int b) { return (a & b) ^ (a | b); }";
        let obj = compile_source(src).unwrap();
        assert_eq!(&obj[..4], b"\x7fELF");
    }

    #[test]
    fn emits_shift_operations_with_independent_integer_promotions() {
        let source = "int left(int x, long count) { return x << count; } \
             unsigned int logical(unsigned int x, int count) { return x >> count; } \
             int arithmetic(int x, unsigned long count) { return x >> count; } \
             int main(void) { return left(3,2)+logical(16,3)+arithmetic(-8,2); }";
        let obj = compile_source(source).unwrap();
        assert_eq!(&obj[..4], b"\x7fELF");
    }

    #[test]
    fn compound_assignments_lower_to_single_lvalue_address() {
        let source = "int calls; int value=7; \
            int *next(void) { calls=calls+1; return &value; } \
            int main(void) { *next() &= 3; *next() |= 8; \
                *next() ^= 2; *next() <<= 1; *next() >>= 1; \
                return (calls == 5) && (value == 9); }";
        let obj = compile_source(source).unwrap();
        assert_eq!(&obj[..4], b"\x7fELF");
    }

    #[test]
    fn compound_assignments_handle_narrow_lvalues_and_chains() {
        let source = "int main(void) { unsigned char byte=255; int x=7; \
            byte &= 15; byte <<= 1; byte >>= 2; \
            x ^= byte |= 1; return x; }";
        assert!(!compile_source(source).unwrap().is_empty());
    }

    #[test]
    fn compiles_arithmetic_compound_assignments_and_pointer_scaled_offsets() {
        let src = "int calls; int a[4]; int *p=a; \
                   int **slot(void) { calls += 1; return &p; } \
                   int main(void) { a[1]=17; a[2]=23; \
                       *slot() += 2; *slot() -= 1; \
                       return calls + *p; }";
        let obj = compile_source(src).unwrap();
        assert_eq!(&obj[..4], b"\x7fELF");
    }

    #[test]
    fn compiles_unsigned_arithmetic_compound_assignments() {
        let src = "int main(void) { unsigned char c=250; int a=12; \
                   c += 10; c *= 3; c /= 2; c %= 7; \
                   a += 2; a -= 1; a *= 2; a /= 3; a %= 5; \
                   return c+a; }";
        assert!(!compile_source(src).unwrap().is_empty());
    }

    #[test]
    fn compiles_prefix_postfix_updates_without_recomputing_lvalue() {
        let source = "int calls; int value=3; \
            int *slot(void) { calls += 1; return &value; } \
            int main(void) { int old=(*slot())++; \
                int after=++*slot(); \
                return old+after+calls; }";
        let obj = compile_source(source).unwrap();
        assert_eq!(&obj[..4], b"\x7fELF");
    }

    #[test]
    fn compiles_pointer_increment_decrement_and_narrow_assignment_values() {
        let source = "struct Pair { int x; int y; }; \
            int main(void) { struct Pair a[3]; \
                struct Pair *p=a; p++; ++p; p--; \
                unsigned char c=0; int saved=(c=260); \
                return saved+c; }";
        assert!(!compile_source(source).unwrap().is_empty());
    }

    #[test]
    fn compiles_for_loops_with_continue_increment_and_nested_breaks() {
        let sources = [
            "int main(void) { int s=0; for(int i=0;i<5;i++) { if(i==2)continue; s+=i; } return s; }",
            "int main(void) { int i=0; for(;;) { i++; if(i==3)break; } return i; }",
            "int main(void) { int s=0; for(int i=0;i<3;i++) for(int j=0;j<4;j++) { if(j==2)continue; if(i==2)break; s+=i+j; } return s; }",
            "int main(void) { int i=7; for(int i=0;i<4;i++) {} return i; }",
        ];
        for source in sources {
            let obj = compile_source(source).unwrap();
            assert_eq!(&obj[..4], b"\x7fELF");
        }
    }

    #[test]
    fn compiles_comma_sequence_with_for_and_function_arguments() {
        let sources = [
            "int main(void) { int i=0; int j=0; int s=0; for (i=0,j=0; i<4; i++,j+=2) s+=i+j; return s; }",
            "int f(int a,int b) { return a+b; } int main(void) { int x=0; return f((x=3,x),4); }",
            "int main(void) { int x=0; int *p=0; return *(x=7,p=&x,p); }",
            "void mark(void) { return; } int main(void) { return (mark(), 7); }",
        ];
        for source in sources {
            let object = compile_source(source).unwrap();
            assert_eq!(&object[..4], b"\x7fELF");
        }
    }

    #[test]
    fn compiles_do_while_post_test_and_continue_targets() {
        let sources = [
            "int main(void) { int x=0; do { x++; } while(0); return x; }",
            "int main(void) { int i=0; int checks=0; do { i++; if(i==1)continue; } while (checks++, i<3); return i*10+checks; }",
            "int main(void) { int i=0; do { ++i; if(i==1)break; } while(i<10); return i; }",
            "int main(void) { int sum=0; for(int i=0;i<3;i++) { int j=0; do { ++j; if(j==2)continue; sum+=i+j; } while(j<3); } return sum; }",
        ];
        for source in sources {
            let object = compile_source(source).unwrap();
            assert_eq!(&object[..4], b"\x7fELF");
        }
    }

    #[test]
    fn compiles_lazy_conditional_integer_pointer_and_void_branches() {
        let sources = [
            "int main(void) { int x=0; int y=1 ? ++x : ++x; return y+x; }",
            "int main(void) { int x=2; return 0 ? 20/(x-x) : 9; }",
            "int main(void) { int x=4; int *p=1 ? &x : 0; return *p; }",
            "void touch(void) { return; } int main(void) { 1 ? touch() : touch(); return 0; }",
            "int main(void) { int c=0; return c ? 2 : c ? 4 : 7; }",
        ];
        for source in sources {
            let object = compile_source(source).unwrap();
            assert_eq!(&object[..4], b"\x7fELF");
        }
    }


    #[test]
    fn m10_native_float_literal_is_explicitly_unsupported() {
        // Parse and check the source, but never misemit its FP value as an
        // integer or use a fabricated SysV return convention.
        assert!(compile_source("float f(void) { return 0.5f; }").is_err());
        assert!(compile_source(
            "double f(void) { return 3.40282346638528859811704183484516925e+38; }"
        ).is_err());
    }

}
