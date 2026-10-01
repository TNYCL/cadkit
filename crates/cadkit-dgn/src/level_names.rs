//! Shared, deterministic neutral names for V8 level IDs in table order.

use std::collections::HashSet;

/// Assigns a unique name, including when a literal name resembles a generated alias.
/// The table walker has already bounded the number of levels by the read limits.
pub(crate) fn assign(name: Option<&str>, id: u32, used: &mut HashSet<String>) -> String {
    let base = name
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Level {id}"));
    let mut candidate = base.clone();
    // At most used.len() candidates can be occupied. Bounded probing also handles
    // a real level named, for example, "Shared (2)" before the second "Shared".
    for suffix in 0..=used.len() {
        if !used.contains(&candidate) {
            break;
        }
        candidate = if suffix == 0 {
            format!("{base} ({id})")
        } else {
            format!("{base} ({id}, {suffix})")
        };
    }
    used.insert(candidate.clone());
    candidate
}
