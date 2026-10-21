//! Optional, backend-only Cranelift object emitter for typed Mini-C functions.
//! No dependency from syntax or sema to Cranelift, and no TU-sized AST.
//!
//! Deliberately supports only integer scalar functions, local variables,
//! control flow, and direct calls. Unsupported features fail explicitly.
use cranelift_codegen::ir::{self, condcodes::IntCC, types, AbiParam, InstBuilder, Value};
use cranelift_codegen::entity::EntityRef;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{default_libcall_names, FuncId, Linkage as ClLinkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use mini_c_sema::{
    CType, CheckedDecl, CheckedExpr, CheckedExprKind, CheckedFunction,
    CheckedItem, CheckedStmt, Checker, IntRank, Linkage, Target,
};
use mini_c_syntax::{ast::{BinOp, UnOp, StorageClass}, parser::Parser, stream::ItemSource};
use std::collections::HashMap;

type Result<T> = std::result::Result<T, String>;

fn scalar_type(ty: &CType) -> Result<ir::Type> {
    match ty {
        CType::Integer { rank: IntRank::Int, .. } => Ok(types::I32),
        CType::Integer { rank: IntRank::Long, .. } => Ok(types::I64),
        _ => Err(format!("M2.5 only supports int/long scalar values, got {ty:?}")),
    }
}

fn signature(module: &ObjectModule, ty: &CType) -> Result<ir::Signature> {
    let CType::Function { params, result } = ty else {
        return Err("function declaration requires a function type".into());
    };
    let mut sig = module.make_signature();
    for param in params {
        sig.params.push(AbiParam::new(scalar_type(param)?));
    }
    if result.as_ref() != &CType::Void {
        sig.returns.push(AbiParam::new(scalar_type(result)?));
    }
    Ok(sig)
}

#[derive(Clone)]
struct KnownFunction {
    id: FuncId,
    ty: CType,
    defined: bool,
}

/// One object module is built incrementally. Completed checked functions are
/// compiled and discarded immediately; only named signatures/FuncIds survive.
pub struct Compiler {
    module: ObjectModule,
    functions: HashMap<String, KnownFunction>,
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
        Ok(Self { module: ObjectModule::new(object), functions: HashMap::new() })
    }

    fn declare(&mut self, name: &str, ty: &CType, linkage: Linkage, definition: bool) -> Result<FuncId> {
        if let Some(known) = self.functions.get(name) {
            if &known.ty != ty {
                return Err(format!("conflicting backend signature for '{name}'"));
            }
            if definition && known.defined {
                return Err(format!("duplicate backend definition for '{name}'"));
            }
            if !definition {
                return Ok(known.id);
            }
        }
        let linkage = match (linkage, definition) {
            (Linkage::Internal, _) => ClLinkage::Local,
            (_, true) => ClLinkage::Export,
            (_, false) => ClLinkage::Import,
        };
        let sig = signature(&self.module, ty)?;
        let id = self.module.declare_function(name, linkage, &sig)
            .map_err(|e| format!("declaring '{name}': {e}"))?;
        self.functions.insert(name.to_owned(), KnownFunction {
            id, ty: ty.clone(), defined: definition,
        });
        Ok(id)
    }

    /// Register an external declaration or compile one complete checked function.
    /// File-scope object storage/pointers are an explicit later milestone.
    pub fn add(&mut self, item: CheckedItem) -> Result<()> {
        match item {
            CheckedItem::Typedef { .. } => Ok(()),
            CheckedItem::Declaration(decl) => {
                if !matches!(decl.ty, CType::Function { .. }) {
                    return Err(format!("M2.5 does not emit global object '{}'", decl.name));
                }
                self.declare(&decl.name, &decl.ty, decl.linkage, false)?;
                Ok(())
            }
            CheckedItem::Function(function) => self.define(function),
        }
    }

    fn define(&mut self, function: CheckedFunction) -> Result<()> {
        let id = self.declare(&function.name, &function.signature, function.linkage, true)?;
        let mut ctx = self.module.make_context();
        ctx.func.signature = signature(&self.module, &function.signature)?;
        ctx.func.name = ir::UserFuncName::user(0, id.as_u32());
        let config = self.module.target_config();
        {
            let mut fb_ctx = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);
            let mut lower = Lower {
                module: &mut self.module, functions: &self.functions,
                scopes: vec![HashMap::new()], loops: Vec::new(),
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
                if lower.result_ty == CType::Void {
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

    pub fn finish(self) -> Result<Vec<u8>> {
        self.module.finish().emit().map_err(|e| e.to_string())
    }
}

/// Compile using the same incremental Parser + Checker as the normal driver.
/// The source string is currently buffered; function ASTs are not retained.
pub fn compile_source(source: &str) -> Result<Vec<u8>> {
    let mut parser = Parser::new(source);
    let mut checker = Checker::new(Target::default()).map_err(|e| e.message)?;
    let mut compiler = Compiler::new()?;
    while let Some(item) = parser.next_item().map_err(|e| e.message)? {
        let checked = checker.check_item(item).map_err(|e| e.message)?;
        compiler.add(checked)?;
    }
    compiler.finish()
}

#[derive(Clone, Copy)]
struct Local {
    variable: Variable,
}

struct Lower<'a> {
    module: &'a mut ObjectModule,
    functions: &'a HashMap<String, KnownFunction>,
    scopes: Vec<HashMap<String, Local>>,
    loops: Vec<(ir::Block, ir::Block)>, // (break target, continue target)
    result_ty: CType,
}

impl Lower<'_> {
    fn bind(&mut self, b: &mut FunctionBuilder<'_>, name: &str, ty: &CType, value: Option<Value>) -> Result<()> {
        let clty = scalar_type(ty)?;
        let var = b.declare_var(clty);
        // A read before initialization is undefined C; zero is a deterministic
        // backend default, not a language guarantee.
        let init = match value { Some(v) => v, None => b.ins().iconst(clty, 0) };
        b.def_var(var, init);
        self.scopes.last_mut().expect("scope exists").insert(name.to_string(), Local { variable: var });
        Ok(())
    }

    fn variable(&self, name: &str) -> Result<Variable> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
            .map(|local| local.variable)
            .ok_or_else(|| format!("M2.5 does not support global objects or unresolved variable '{name}'"))
    }

    fn integer_value(&mut self, b: &mut FunctionBuilder<'_>, expr: &CheckedExpr) -> Result<Value> {
        let result = self.expression(b, expr)?;
        if !expr.ty.is_integer() {
            return Err(format!("M2.5 integer expression expected, got {:?}", expr.ty));
        }
        Ok(result)
    }

    fn condition(&mut self, b: &mut FunctionBuilder<'_>, expr: &CheckedExpr) -> Result<Value> {
        let value = self.integer_value(b, expr)?;
        Ok(b.ins().icmp_imm(IntCC::NotEqual, value, 0))
    }

    fn boolean(b: &mut FunctionBuilder<'_>, value: Value) -> Value {
        let one = b.ins().iconst(types::I32, 1);
        let zero = b.ins().iconst(types::I32, 0);
        b.ins().select(value, one, zero)
    }

    fn convert(b: &mut FunctionBuilder<'_>, value: Value, from: &CType, to: &CType) -> Result<Value> {
        let src_ty = scalar_type(from)?;
        let dst_ty = scalar_type(to)?;
        if src_ty == dst_ty { return Ok(value); }
        if src_ty.bits() < dst_ty.bits() {
            let signed = matches!(from, CType::Integer { unsigned: false, .. });
            Ok(if signed { b.ins().sextend(dst_ty, value) } else { b.ins().uextend(dst_ty, value) })
        } else {
            Ok(b.ins().ireduce(dst_ty, value))
        }
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
            CheckedExprKind::Literal(n) => Ok(b.ins().iconst(scalar_type(&expr.ty)?, *n as i64)),
            CheckedExprKind::Name(name) => Ok(b.use_var(self.variable(name)?)),
            CheckedExprKind::Load(inner) => self.expression(b, inner),
            CheckedExprKind::Convert(inner) => {
                let v = self.expression(b, inner)?;
                Self::convert(b, v, &inner.ty, &expr.ty)
            }
            CheckedExprKind::Unary(op, inner) => {
                let val = self.integer_value(b, inner)?;
                match op {
                    UnOp::Plus => Ok(val),
                    UnOp::Neg => Ok(b.ins().ineg(val)),
                    UnOp::Not => {
                        let cond = b.ins().icmp_imm(IntCC::Equal, val, 0);
                        Ok(Self::boolean(b, cond))
                    }
                    UnOp::Address | UnOp::Deref => Err("M2.5 pointer operations are unsupported".into()),
                }
            }
            CheckedExprKind::Assign(lhs, rhs) => {
                let CheckedExprKind::Name(name) = &lhs.kind else {
                    return Err("M2.5 assignment supports local variables only".into());
                };
                let rhs_value = self.integer_value(b, rhs)?;
                b.def_var(self.variable(name)?, rhs_value);
                Ok(rhs_value)
            }
            CheckedExprKind::Binary(op, lhs, rhs) => {
                if matches!(op, BinOp::And | BinOp::Or) {
                    return self.short_circuit(b, *op, lhs, rhs);
                }
                let left = self.integer_value(b, lhs)?;
                let right = self.integer_value(b, rhs)?;
                let unsigned = matches!(&lhs.ty, CType::Integer { unsigned: true, .. });
                let value = match op {
                    BinOp::Add => b.ins().iadd(left, right),
                    BinOp::Sub => b.ins().isub(left, right),
                    BinOp::Mul => b.ins().imul(left, right),
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
                let CheckedExprKind::Name(name) = &callee.kind else {
                    return Err("M2.5 only supports direct named function calls".into());
                };
                let known = self.functions.get(name)
                    .ok_or_else(|| format!("M2.5 missing function declaration for '{name}'"))?;
                let mut values = Vec::with_capacity(args.len());
                for arg in args { values.push(self.integer_value(b, arg)?); }
                let refid = self.module.declare_func_in_func(known.id, b.func);
                let inst = b.ins().call(refid, &values);
                let values = b.inst_results(inst);
                if let Some(&result) = values.first() {
                    Ok(result)
                } else {
                    // All valid uses of a void function are statement expressions.
                    Ok(b.ins().iconst(types::I32, 0))
                }
            }
        }
    }

    /// Returns whether control can fall through to the next statement.
    fn statement(&mut self, b: &mut FunctionBuilder<'_>, stmt: &CheckedStmt) -> Result<bool> {
        match stmt {
            CheckedStmt::Block(stmts) => {
                self.scopes.push(HashMap::new());
                let mut live = true;
                for stmt in stmts {
                    if !live { break; }
                    live = self.statement(b, stmt)?;
                }
                self.scopes.pop();
                Ok(live)
            }
            CheckedStmt::Typedef { .. } => Ok(true),
            CheckedStmt::Decl(CheckedDecl { name, ty, storage, initializer, .. }) => {
                if *storage != StorageClass::Default {
                    return Err(format!("M2.5 does not emit storage-class local '{name}'"));
                }
                let value = initializer.as_ref().map(|e| self.integer_value(b, e)).transpose()?;
                self.bind(b, name, ty, value)?;
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
                b.switch_to_block(loop_body);
                let body_live = self.statement(b, body)?;
                self.loops.pop();
                if body_live { b.ins().jump(header, &[]); }
                b.switch_to_block(exit);
                Ok(true)
            }
            CheckedStmt::Break => {
                let (break_dest, _) = self.loops.last()
                    .copied().ok_or("break outside loop in checked IR")?;
                b.ins().jump(break_dest, &[]);
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
    fn handles_unsigned_and_long_conversions() {
        let src = "unsigned long a(unsigned long x, unsigned int y) { \
                   return x + y * 2; } int main(void) { return a(50,3) == 56; }";
        assert!(!compile_source(src).unwrap().is_empty());
    }

    #[test]
    fn backend_rejects_unsupported_global_objects_and_pointers() {
        let error = compile_source("int global = 3; int main(void) { return global; }").unwrap_err();
        assert!(error.contains("global object"), "{error}");
        let error = compile_source("int main(void) { int x = 2; int *p = &x; return *p; }").unwrap_err();
        assert!(error.contains("pointer"), "{error}");
    }

    #[test]
    fn rejects_fallthrough_in_non_main_nonvoid() {
        let error = compile_source("int f(int x) { if (x) return x; }").unwrap_err();
        assert!(error.contains("may reach its end"), "{error}");
        assert!(!compile_source("int main(void) { int x = 1; }").unwrap().is_empty());
    }

    #[test]
    fn accepts_prototypes_before_definition_and_externals() {
        let src = "int add(int, int); int main(void) { return add(3,4); }\
                   int add(int x, int y) { return x + y; }";
        assert!(!compile_source(src).unwrap().is_empty());
    }
}
