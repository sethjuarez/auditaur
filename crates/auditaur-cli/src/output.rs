pub fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::bounded_text;

    #[test]
    fn truncates_on_character_boundaries() {
        assert_eq!(bounded_text("a🦀b", 2), "a🦀");
    }
}
