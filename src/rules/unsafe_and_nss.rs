//! Every `unsafe` block has a `// SAFETY:` comment; NSS allocations use RAII
//!
//! Docs and examples: `rules/unsafe-and-nss/`.

use std::collections::BTreeSet;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::helpers::idioms::{self, functions};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "unsafe-and-nss",
    area: Area::Idioms,
    title: "Every `unsafe` block has a `// SAFETY:` comment; NSS allocations use RAII",
    kind: Kind::Code,
    clippy: &["clippy::undocumented_unsafe_blocks"],
    in_tests: false,
    check,
};

/// NSS functions that free or destroy what an earlier call allocated.
const NSS_FREES: &[&str] = &[
    "SECITEM_FreeItem",
    "SECITEM_ZfreeItem",
    "PORT_Free",
    "PORT_ZFree",
    "PORT_FreeArena",
    "PK11_FreeSlot",
    "PK11_FreeSymKey",
    "PK11_DestroyContext",
    "PK11_DestroyGenericObject",
    "SECKEY_DestroyPrivateKey",
    "SECKEY_DestroyPublicKey",
    "SECKEY_DestroySubjectPublicKeyInfo",
    "SECKEY_DestroyEncryptedPrivateKeyInfo",
    "SECOID_DestroyAlgorithmID",
    "CERT_DestroyCertificate",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws) {
        undocumented_unsafe(file, out);
        manual_frees(file, out);
        if file.path.contains("/nss_sys/") {
            pkcs11_abi(file, out);
        }
    }
}

/// `unsafe { .. }` that dereferences a raw pointer or reinterprets memory,
/// with no `SAFETY:` comment above its statement, on its line, or at the top
/// of the block. (Plain FFI calls are left to `undocumented_unsafe_blocks`.)
fn undocumented_unsafe(file: &RustFile, out: &mut Findings) {
    let mut v = UnsafeBlocks {
        stmt_start: 0,
        hits: Vec::new(),
    };
    v.visit_file(&file.ast);
    for (block_line, stmt_line, column) in v.hits {
        if documented(file, block_line, stmt_line) {
            continue;
        }
        out.add_at(
            &file.path,
            block_line,
            column,
            "`unsafe` block reads raw memory without a `// SAFETY:` comment; say why the pointer is valid and the data outlives its use",
        );
    }
}

/// `(line of the block, first line of its statement, column)` per unsafe block.
struct UnsafeBlocks {
    stmt_start: usize,
    hits: Vec<(usize, usize, usize)>,
}

impl<'a> Visit<'a> for UnsafeBlocks {
    fn visit_stmt(&mut self, stmt: &'a syn::Stmt) {
        let outer = self.stmt_start;
        self.stmt_start = stmt.span().start().line;
        syn::visit::visit_stmt(self, stmt);
        self.stmt_start = outer;
    }

    fn visit_expr_unsafe(&mut self, e: &'a syn::ExprUnsafe) {
        let start = e.unsafe_token.span.start();
        let stmt = if self.stmt_start == 0 {
            start.line
        } else {
            self.stmt_start
        };
        let mut ops = RawOps(false);
        ops.visit_block(&e.block);
        if ops.0 {
            self.hits.push((start.line, stmt, start.column + 1));
        }
        syn::visit::visit_expr_unsafe(self, e);
    }

    fn visit_item_fn(&mut self, f: &'a syn::ItemFn) {
        let outer = self.stmt_start;
        self.stmt_start = 0;
        syn::visit::visit_item_fn(self, f);
        self.stmt_start = outer;
    }
}

/// Functions that build references, slices or values out of raw memory.
const RAW_MEMORY_FNS: &[&str] = &[
    "from_raw_parts",
    "from_raw_parts_mut",
    "transmute",
    "transmute_copy",
    "zeroed",
    "assume_init",
    "get_unchecked",
    "get_unchecked_mut",
    "set_len",
    "copy_nonoverlapping",
    "from_utf8_unchecked",
    "sec_item_as_slice",
];

/// Whether a block does more than call FFI: dereferences a raw pointer or
/// reinterprets memory, where the soundness argument is not obvious.
struct RawOps(bool);

impl<'a> Visit<'a> for RawOps {
    fn visit_expr_unary(&mut self, e: &'a syn::ExprUnary) {
        if matches!(e.op, syn::UnOp::Deref(_)) {
            self.0 = true;
        }
        syn::visit::visit_expr_unary(self, e);
    }

    fn visit_expr_call(&mut self, call: &'a syn::ExprCall) {
        if let syn::Expr::Path(p) = &*call.func {
            let last = p.path.segments.last().map(|s| s.ident.to_string());
            if last.is_some_and(|l| RAW_MEMORY_FNS.contains(&l.as_str()))
                || syntax::path_ends_with(&p.path, "CStr::from_ptr")
                || syntax::path_ends_with(&p.path, "ptr::read")
                || syntax::path_ends_with(&p.path, "ptr::write")
            {
                self.0 = true;
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, m: &'a syn::ExprMethodCall) {
        if RAW_MEMORY_FNS.contains(&m.method.to_string().as_str()) {
            self.0 = true;
        }
        syn::visit::visit_expr_method_call(self, m);
    }
}

fn documented(file: &RustFile, block_line: usize, stmt_line: usize) -> bool {
    let safety = |line: usize| file.line(line).to_lowercase().contains("safety:");
    // On the statement's lines, up to the line after `unsafe {`.
    if (stmt_line..=block_line + 1).any(safety) {
        return true;
    }
    // In the comment lines right above the statement (attributes allowed).
    let mut line = stmt_line;
    while line > 1 {
        line -= 1;
        let text = file.line(line).trim();
        if text.starts_with("//") || text.starts_with("#[") || text.starts_with('*') || text.starts_with("/*") {
            if safety(line) {
                return true;
            }
            continue;
        }
        break;
    }
    false
}

/// A call to an NSS free function after a `?` or `return` in the same
/// function: an early exit skips it and leaks. Destructors (`Drop`, the
/// functions handed to `scoped_ptr!`) are where frees belong.
fn manual_frees(file: &RustFile, out: &mut Findings) {
    let mut destructors = BTreeSet::new();
    syntax::macros(&file.ast, |mac| {
        if syntax::macro_name(mac) == "scoped_ptr" {
            destructors.extend(idioms::idents(&mac.tokens));
        }
    });
    for f in functions(&file.ast) {
        if f.trait_name.as_deref() == Some("Drop") || destructors.contains(&f.name) {
            continue;
        }
        let mut v = Frees {
            first_exit: None,
            frees: Vec::new(),
        };
        v.visit_block(f.block);
        let Some(exit) = v.first_exit else {
            continue;
        };
        for (name, line, column) in v.frees {
            if line > exit {
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!("`{name}` is skipped when the `?`/`return` on line {exit} exits early, leaking the allocation; own it with a `scoped_ptr!` type such as `ScopedSECItem`"),
                );
            }
        }
    }
}

struct Frees {
    first_exit: Option<usize>,
    frees: Vec<(String, usize, usize)>,
}

impl Frees {
    fn exit_at(&mut self, line: usize) {
        self.first_exit = Some(self.first_exit.map_or(line, |l| l.min(line)));
    }
}

impl<'a> Visit<'a> for Frees {
    fn visit_expr_try(&mut self, e: &'a syn::ExprTry) {
        self.exit_at(e.question_token.span.start().line);
        syn::visit::visit_expr_try(self, e);
    }

    fn visit_expr_return(&mut self, e: &'a syn::ExprReturn) {
        self.exit_at(e.return_token.span.start().line);
        syn::visit::visit_expr_return(self, e);
    }

    fn visit_expr_call(&mut self, call: &'a syn::ExprCall) {
        if let syn::Expr::Path(p) = &*call.func {
            if let Some(last) = p.path.segments.last() {
                let name = last.ident.to_string();
                if NSS_FREES.contains(&name.as_str()) {
                    let start = p.path.segments[0].ident.span().start();
                    self.frees.push((name, start.line, start.column + 1));
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    // `?` inside a closure returns from the closure, not the function.
    fn visit_expr_closure(&mut self, _: &'a syn::ExprClosure) {}
}

/// PKCS#11 declarations that break the Windows ABI, where `CK_ULONG` is 32
/// bits and structs are packed.
fn pkcs11_abi(file: &RustFile, out: &mut Findings) {
    for item in syntax::all_items(&file.ast) {
        match item {
            syn::Item::Const(c) if is_pkcs11_name(&c.ident.to_string()) => {
                if let Some(ty) = wide_int(&c.ty) {
                    let (line, column) = file.pos(&c.ty);
                    out.add_at(
                        &file.path,
                        line,
                        column,
                        format!("PKCS#11 constant typed `{ty}`; use `CK_ULONG` (32-bit on Windows)"),
                    );
                }
            }
            syn::Item::Struct(s) if s.ident.to_string().starts_with("CK_") => {
                let packed = s.attrs.iter().any(|a| {
                    a.path().is_ident("cfg_attr") && {
                        let args = syntax::attr_args(a);
                        args.contains("windows") && args.contains("packed")
                    }
                });
                if !packed {
                    let (line, column) = file.pos(&s.ident);
                    out.add_at(
                        &file.path,
                        line,
                        column,
                        "PKCS#11 struct without `#[cfg_attr(target_os = \"windows\", repr(C, packed))]`; Windows NSS packs these structs",
                    );
                }
                for field in &s.fields {
                    if let Some(ty) = wide_int(&field.ty) {
                        let (line, column) = file.pos(&field.ty);
                        out.add_at(
                            &file.path,
                            line,
                            column,
                            format!("PKCS#11 field typed `{ty}`; use `CK_ULONG` (32-bit on Windows)"),
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_pkcs11_name(name: &str) -> bool {
    ["CKM_", "CKA_", "CKK_", "CKO_", "CKR_", "CKF_", "CKD_", "CKG_", "CKZ_"]
        .iter()
        .any(|p| name.starts_with(p))
}

fn wide_int(ty: &syn::Type) -> Option<String> {
    let t = syntax::type_string(ty);
    matches!(t.as_str(), "u64" | "i64" | "std::os::raw::c_ulonglong").then_some(t)
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/support/rc_crypto/nss/src/pk11/sym_key.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/unsafe-and-nss/dont.rs"))],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![4, 5], "{found:#?}");
        assert!(found[1].message.contains("`SECITEM_FreeItem` is skipped"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/unsafe-and-nss/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn accepts_frees_in_destructors_and_comments_above_statements() {
        let src = "scoped_ptr!(SymKey, PK11SymKey, pk11_free_sym_key);\n\
            unsafe fn pk11_free_sym_key(k: *mut PK11SymKey) { check(k)?; nss_sys::PK11_FreeSymKey(k); }\n\
            impl Drop for ScopedSECItem {\n\
                fn drop(&mut self) {\n\
                    // SAFETY: we own `wrapped`.\n\
                    unsafe { nss_sys::SECITEM_FreeItem(&mut self.wrapped, nss_sys::PR_FALSE) };\n\
                }\n\
            }\n\
            fn f() -> Result<()> {\n\
                // SAFETY: NSS returns a valid OID entry for every known tag.\n\
                let len =\n\
                    unsafe { (*oid_data).oid.len };\n\
                let code = unsafe { nss_sys::PR_GetError() };\n\
                Ok(())\n\
            }\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_windows_abi_breaks_in_nss_sys() {
        let src = "pub const CKM_AES_KEY_GEN: u64 = 0x1080;\n\
            pub const CKM_AES_CBC: CK_ULONG = 0x1082;\n\
            #[repr(C)]\npub struct CK_GCM_PARAMS { pub ulIvLen: CK_ULONG }\n";
        let found = check(
            &RULE,
            &[("components/support/rc_crypto/nss/nss_sys/src/bindings/pkcs11t.rs", src)],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 1);
        assert_eq!(found[1].line, 4);
    }
}
