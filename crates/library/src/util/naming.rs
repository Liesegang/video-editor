use std::collections::HashSet;

/// Preserves a readable base name while choosing the first unused numeric suffix.
pub fn unique_name<'a>(base: &str, existing: impl IntoIterator<Item = &'a str>) -> String {
    let existing = existing.into_iter().collect::<HashSet<_>>();
    if !existing.contains(base) {
        return base.to_string();
    }
    (2..)
        .map(|suffix| format!("{base} {suffix}"))
        .find(|candidate| !existing.contains(candidate.as_str()))
        .unwrap_or_else(|| format!("{base} Copy"))
}

#[cfg(test)]
mod tests {
    use super::unique_name;

    #[test]
    fn names_are_unique_without_discarding_the_readable_base() {
        assert_eq!(unique_name("Composition", ["Main", "Title"]), "Composition");
        assert_eq!(
            unique_name("Composition", ["Composition", "Composition 2"]),
            "Composition 3"
        );
    }
}
