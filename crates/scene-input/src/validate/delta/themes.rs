//! Property resolution under every theme, on the affected styles and nodes.
use super::affected::Affected;
use crate::validate::{scope::Resolver, Lookup};
use crate::Result;
use std::collections::BTreeSet;

/// `default` and every theme a token declares: the themes `validate_tokens` returns on
/// a valid token set.
pub(super) fn declared(after: &dyn Lookup) -> BTreeSet<String> {
    let mut themes = BTreeSet::from(["default".to_owned()]);
    themes.extend(after.declared_themes().into_iter().map(str::to_owned));
    themes
}

/// Per theme, the affected styles then the affected nodes, each in id order.
pub(super) fn check(
    after: &dyn Lookup,
    affected: &Affected,
    themes: &BTreeSet<String>,
) -> Result<()> {
    for theme in themes {
        let resolver = Resolver {
            lookup: after,
            overlay: None,
            theme,
        };
        for style in affected.styles.iter().filter_map(|id| after.style(id)) {
            resolver.style(style)?;
        }
        for node in affected.nodes.iter().filter_map(|id| after.node(id)) {
            resolver.node(node)?;
        }
    }
    Ok(())
}
