//! Emit a fact document for a Rust source tree.
//!
//! The Python adherence checks were written against Python's `ast`. Rust has no
//! such module on the Python side, so this binary supplies the same facts the
//! predicates need — item shapes, call sites, macro invocations, and the scope
//! each one sits in — as JSON.
//!
//! The point of the exercise is to find out which parts of the check design
//! survive a change of parser. So this emits *facts*, not verdicts: no judgement
//! about whether an `unwrap` is acceptable lives here.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use quote::ToTokens;
use serde::Serialize;
use syn::visit::Visit;

/// One item definition: function, type, trait, impl block, or module.
#[derive(Serialize, Default)]
struct Item {
    kind: String,
    name: String,
    path: String,
    line: usize,
    visibility: String,
    is_async: bool,
    doc: String,
    attrs: Vec<String>,
    in_test_scope: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    return_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    impl_trait: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    impl_type: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    variants: Vec<Variant>,
}

/// One enum variant, with the arity of the context it carries.
#[derive(Serialize)]
struct Variant {
    name: String,
    fields: usize,
}

/// One call site. `name` is the method or final path segment.
#[derive(Serialize)]
struct Call {
    name: String,
    full: String,
    line: usize,
    in_item: String,
    in_test_scope: bool,
}

/// One macro invocation, which in Rust is where `panic!` and `println!` live.
#[derive(Serialize)]
struct MacroCall {
    name: String,
    line: usize,
    in_item: String,
    in_test_scope: bool,
    arguments: String,
}

/// Everything one source file contributes.
#[derive(Serialize, Default)]
struct FileFacts {
    path: String,
    crate_doc: String,
    crate_attrs: Vec<String>,
    uses: Vec<String>,
    items: Vec<Item>,
    calls: Vec<Call>,
    macros: Vec<MacroCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_error: Option<String>,
}

#[derive(Serialize)]
struct Document {
    root: String,
    files: Vec<FileFacts>,
}

struct Collector {
    facts: FileFacts,
    scope: Vec<String>,
    test_depth: usize,
}

impl Collector {
    fn new(path: String) -> Self {
        Self {
            facts: FileFacts {
                path,
                ..FileFacts::default()
            },
            scope: Vec::new(),
            test_depth: 0,
        }
    }

    fn current_item(&self) -> String {
        self.scope.join("::")
    }

    fn in_test_scope(&self) -> bool {
        self.test_depth > 0
    }

    fn record(&mut self, item: Item) {
        self.facts.items.push(item);
    }
}

fn attribute_strings(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("doc"))
        .map(|attr| attr.meta.to_token_stream().to_string())
        .collect()
}

fn doc_string(attrs: &[syn::Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("doc")) {
        if let syn::Meta::NameValue(value) = &attr.meta {
            if let syn::Expr::Lit(expr) = &value.value {
                if let syn::Lit::Str(text) = &expr.lit {
                    lines.push(text.value().trim().to_string());
                }
            }
        }
    }
    lines.join("\n")
}

/// Is this definition gated behind `#[cfg(test)]`?
fn is_test_gated(attrs: &[syn::Attribute]) -> bool {
    attribute_strings(attrs)
        .iter()
        .any(|attr| attr.starts_with("cfg") && attr.contains("test"))
}

fn visibility_string(visibility: &syn::Visibility) -> String {
    match visibility {
        syn::Visibility::Public(_) => "pub".to_string(),
        syn::Visibility::Restricted(restricted) => {
            format!("pub({})", restricted.path.to_token_stream())
        }
        syn::Visibility::Inherited => "inherited".to_string(),
    }
}

fn type_string(ty: &syn::ReturnType) -> Option<String> {
    match ty {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_, inner) => Some(inner.to_token_stream().to_string()),
    }
}

fn path_tail(path: &syn::Path) -> String {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default()
}

impl<'ast> Visit<'ast> for Collector {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let gated = is_test_gated(&node.attrs);
        self.record(Item {
            kind: "mod".to_string(),
            name: node.ident.to_string(),
            path: join(&self.scope, &node.ident.to_string()),
            line: node.ident.span().start().line,
            visibility: visibility_string(&node.vis),
            doc: doc_string(&node.attrs),
            attrs: attribute_strings(&node.attrs),
            in_test_scope: self.in_test_scope() || gated,
            ..Item::default()
        });

        self.scope.push(node.ident.to_string());
        if gated {
            self.test_depth += 1;
        }
        syn::visit::visit_item_mod(self, node);
        if gated {
            self.test_depth -= 1;
        }
        self.scope.pop();
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let gated = is_test_gated(&node.attrs);
        self.record(Item {
            kind: "fn".to_string(),
            name: node.sig.ident.to_string(),
            path: join(&self.scope, &node.sig.ident.to_string()),
            line: node.sig.ident.span().start().line,
            visibility: visibility_string(&node.vis),
            is_async: node.sig.asyncness.is_some(),
            doc: doc_string(&node.attrs),
            attrs: attribute_strings(&node.attrs),
            in_test_scope: self.in_test_scope() || gated,
            return_type: type_string(&node.sig.output),
            ..Item::default()
        });

        self.scope.push(node.sig.ident.to_string());
        if gated {
            self.test_depth += 1;
        }
        syn::visit::visit_item_fn(self, node);
        if gated {
            self.test_depth -= 1;
        }
        self.scope.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.record(Item {
            kind: "fn".to_string(),
            name: node.sig.ident.to_string(),
            path: join(&self.scope, &node.sig.ident.to_string()),
            line: node.sig.ident.span().start().line,
            visibility: visibility_string(&node.vis),
            is_async: node.sig.asyncness.is_some(),
            doc: doc_string(&node.attrs),
            attrs: attribute_strings(&node.attrs),
            in_test_scope: self.in_test_scope(),
            return_type: type_string(&node.sig.output),
            ..Item::default()
        });

        self.scope.push(node.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.record(Item {
            kind: "trait_fn".to_string(),
            name: node.sig.ident.to_string(),
            path: join(&self.scope, &node.sig.ident.to_string()),
            line: node.sig.ident.span().start().line,
            visibility: "inherited".to_string(),
            is_async: node.sig.asyncness.is_some(),
            doc: doc_string(&node.attrs),
            attrs: attribute_strings(&node.attrs),
            in_test_scope: self.in_test_scope(),
            return_type: type_string(&node.sig.output),
            ..Item::default()
        });
        syn::visit::visit_trait_item_fn(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        self.record(Item {
            kind: "struct".to_string(),
            name: node.ident.to_string(),
            path: join(&self.scope, &node.ident.to_string()),
            line: node.ident.span().start().line,
            visibility: visibility_string(&node.vis),
            doc: doc_string(&node.attrs),
            attrs: attribute_strings(&node.attrs),
            in_test_scope: self.in_test_scope(),
            ..Item::default()
        });
        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let variants = node
            .variants
            .iter()
            .map(|variant| Variant {
                name: variant.ident.to_string(),
                fields: variant.fields.len(),
            })
            .collect();
        self.record(Item {
            kind: "enum".to_string(),
            name: node.ident.to_string(),
            path: join(&self.scope, &node.ident.to_string()),
            line: node.ident.span().start().line,
            visibility: visibility_string(&node.vis),
            doc: doc_string(&node.attrs),
            attrs: attribute_strings(&node.attrs),
            in_test_scope: self.in_test_scope(),
            variants,
            ..Item::default()
        });
        syn::visit::visit_item_enum(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        self.record(Item {
            kind: "trait".to_string(),
            name: node.ident.to_string(),
            path: join(&self.scope, &node.ident.to_string()),
            line: node.ident.span().start().line,
            visibility: visibility_string(&node.vis),
            doc: doc_string(&node.attrs),
            attrs: attribute_strings(&node.attrs),
            in_test_scope: self.in_test_scope(),
            ..Item::default()
        });
        self.scope.push(node.ident.to_string());
        syn::visit::visit_item_trait(self, node);
        self.scope.pop();
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let implemented = node
            .trait_
            .as_ref()
            .map(|(_, path, _)| path.to_token_stream().to_string());
        let target = node.self_ty.to_token_stream().to_string();
        let label = match &implemented {
            Some(name) => format!("impl {name} for {target}"),
            None => format!("impl {target}"),
        };
        self.record(Item {
            kind: "impl".to_string(),
            name: label.clone(),
            path: join(&self.scope, &label),
            line: node.impl_token.span.start().line,
            visibility: "inherited".to_string(),
            doc: doc_string(&node.attrs),
            attrs: attribute_strings(&node.attrs),
            in_test_scope: self.in_test_scope(),
            impl_trait: implemented,
            impl_type: Some(target.clone()),
            ..Item::default()
        });

        self.scope.push(label);
        syn::visit::visit_item_impl(self, node);
        self.scope.pop();
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        self.facts
            .uses
            .push(node.tree.to_token_stream().to_string().replace(' ', ""));
        syn::visit::visit_item_use(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        self.facts.calls.push(Call {
            name: node.method.to_string(),
            full: format!(
                "{}.{}",
                node.receiver.to_token_stream(),
                node.method
            )
            .replace(' ', ""),
            line: node.method.span().start().line,
            in_item: self.current_item(),
            in_test_scope: self.in_test_scope(),
        });
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            self.facts.calls.push(Call {
                name: path_tail(&path.path),
                full: path.path.to_token_stream().to_string().replace(' ', ""),
                line: path.path.segments.last().map_or(0, |s| s.ident.span().start().line),
                in_item: self.current_item(),
                in_test_scope: self.in_test_scope(),
            });
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.facts.macros.push(MacroCall {
            name: path_tail(&node.path),
            line: node.path.segments.last().map_or(0, |s| s.ident.span().start().line),
            in_item: self.current_item(),
            in_test_scope: self.in_test_scope(),
            arguments: node.tokens.to_string(),
        });
        syn::visit::visit_macro(self, node);
    }
}

fn join(scope: &[String], name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{}::{name}", scope.join("::"))
    }
}

fn collect_sources(root: &Path, found: &mut Vec<PathBuf>) {
    let skip: BTreeSet<&str> = ["target", ".git"].into_iter().collect();
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !skip.contains(name.as_str()) {
                collect_sources(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(root) = arguments.next() else {
        eprintln!("usage: rustfacts <root>");
        std::process::exit(2);
    };
    let root = PathBuf::from(root);

    let mut sources = Vec::new();
    collect_sources(&root, &mut sources);
    sources.sort();

    let mut files = Vec::new();
    for path in sources {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path).unwrap_or_default();
        match syn::parse_file(&source) {
            Ok(parsed) => {
                let mut collector = Collector::new(relative);
                collector.facts.crate_doc = doc_string(&parsed.attrs);
                collector.facts.crate_attrs = attribute_strings(&parsed.attrs);
                collector.visit_file(&parsed);
                files.push(collector.facts);
            }
            Err(error) => files.push(FileFacts {
                path: relative,
                parse_error: Some(error.to_string()),
                ..FileFacts::default()
            }),
        }
    }

    let document = Document {
        root: root.to_string_lossy().to_string(),
        files,
    };
    println!("{}", serde_json::to_string_pretty(&document).unwrap_or_default());
}
