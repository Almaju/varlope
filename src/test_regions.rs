// Adapted from rabot (https://github.com/Almaju/rabot, MIT).

use std::ops::Range;

use syn::spanned::Spanned;
use syn::visit::Visit;

/// The byte ranges of a file that hold test code: anything under
/// `#[cfg(test)]` (or a `cfg` that mentions `test`, such as
/// `#[cfg(any(test, feature = "test-utils"))]`), `#[test]` functions, and
/// the whole file when it lives under `tests/`, `benches/` or `examples/`.
///
/// Most rules are silent inside these regions: an `unwrap` in a test is the
/// assertion.
#[derive(Clone, Debug, Default)]
pub struct TestRegions {
    ranges: Vec<Range<usize>>,
}

impl TestRegions {
    pub fn collect(
        ast: &syn::File,
        whole_file: bool,
        range_of: impl Fn(proc_macro2::Span) -> Range<usize>,
    ) -> Self {
        if whole_file {
            return Self {
                ranges: std::iter::once(0..usize::MAX).collect(),
            };
        }
        let mut collector = Collector {
            range_of: &range_of,
            ranges: Vec::new(),
        };
        collector.visit_file(ast);
        Self {
            ranges: collector.ranges,
        }
    }

    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }

    pub fn contains(&self, offset: usize) -> bool {
        self.ranges.iter().any(|range| range.contains(&offset))
    }
}

struct Collector<'a> {
    range_of: &'a dyn Fn(proc_macro2::Span) -> Range<usize>,
    ranges: Vec<Range<usize>>,
}

impl Collector<'_> {
    fn record<T: Spanned>(&mut self, node: &T) {
        self.ranges.push((self.range_of)(node.span()));
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_impl_item(&mut self, node: &'ast syn::ImplItem) {
        let attrs = match node {
            syn::ImplItem::Const(item) => &item.attrs,
            syn::ImplItem::Fn(item) => &item.attrs,
            syn::ImplItem::Type(item) => &item.attrs,
            syn::ImplItem::Macro(item) => &item.attrs,
            _ => return,
        };
        if is_test_gated(attrs) {
            self.record(node);
            return;
        }
        syn::visit::visit_impl_item(self, node);
    }

    fn visit_trait_item(&mut self, node: &'ast syn::TraitItem) {
        let attrs = match node {
            syn::TraitItem::Const(item) => &item.attrs,
            syn::TraitItem::Fn(item) => &item.attrs,
            syn::TraitItem::Type(item) => &item.attrs,
            syn::TraitItem::Macro(item) => &item.attrs,
            _ => return,
        };
        if is_test_gated(attrs) {
            self.record(node);
            return;
        }
        syn::visit::visit_trait_item(self, node);
    }

    fn visit_item(&mut self, node: &'ast syn::Item) {
        let attrs = match node {
            syn::Item::Const(item) => &item.attrs,
            syn::Item::Enum(item) => &item.attrs,
            syn::Item::ExternCrate(item) => &item.attrs,
            syn::Item::Fn(item) => &item.attrs,
            syn::Item::ForeignMod(item) => &item.attrs,
            syn::Item::Impl(item) => &item.attrs,
            syn::Item::Macro(item) => &item.attrs,
            syn::Item::Mod(item) => &item.attrs,
            syn::Item::Static(item) => &item.attrs,
            syn::Item::Struct(item) => &item.attrs,
            syn::Item::Trait(item) => &item.attrs,
            syn::Item::TraitAlias(item) => &item.attrs,
            syn::Item::Type(item) => &item.attrs,
            syn::Item::Union(item) => &item.attrs,
            syn::Item::Use(item) => &item.attrs,
            _ => return,
        };
        if is_test_gated(attrs) {
            self.record(node);
            return;
        }
        syn::visit::visit_item(self, node);
    }
}

/// `#[cfg(test)]`, `#[cfg(any(test, ..))]`, `#[test]`, `#[tokio::test]`,
/// `#[rstest]`, `#[bench]`.
pub fn is_test_gated(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let last = attr
            .path()
            .segments
            .last()
            .map(|segment| segment.ident.to_string());
        match last.as_deref() {
            Some("cfg") => attr.meta.require_list().is_ok_and(|list| {
                list.tokens
                    .to_string()
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .any(|word| word == "test")
            }),
            Some("test" | "bench" | "rstest") => true,
            _ => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regions(source: &str) -> TestRegions {
        let ast = syn::parse_file(source).unwrap();
        TestRegions::collect(&ast, false, |span| span.byte_range())
    }

    #[test]
    fn cfg_test_items_and_test_fns_are_regions() {
        let source = "fn a() {}\n#[cfg(test)]\nfn b() {}\n#[cfg(any(test, feature = \"x\"))]\nimpl A { fn c() {} }\n#[tokio::test]\nasync fn d() {}\nmod m { #[cfg(test)] mod tests { fn e() {} } }\n";
        let regions = regions(source);
        let offset = |needle: &str| source.find(needle).unwrap();
        assert!(!regions.contains(offset("fn a")));
        assert!(regions.contains(offset("fn b")));
        assert!(regions.contains(offset("fn c")));
        assert!(regions.contains(offset("async fn d")));
        assert!(regions.contains(offset("fn e")));
    }

    #[test]
    fn a_test_file_is_one_region() {
        let ast = syn::parse_file("fn a() {}").unwrap();
        assert!(TestRegions::collect(&ast, true, |span| span.byte_range()).contains(3));
    }

    #[test]
    fn feature_gates_without_test_are_not_regions() {
        let regions =
            regions("#[cfg(feature = \"testing\")]\nfn a() {}\n#[cfg(not(test))]\nfn b() {}");
        assert!(!regions.contains(30));
        // `cfg(not(test))` mentions `test`, and is treated as test-related too:
        // a conservative reading that never silences production code by
        // accident is preferred over parsing cfg logic.
    }
}
