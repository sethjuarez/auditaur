pub fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub fn table_cell(value: &str, max_chars: usize) -> String {
    bounded_text(value, max_chars)
        .chars()
        .map(|ch| match ch {
            '\t' | '\r' | '\n' => ' ',
            _ => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{bounded_text, table_cell};

    #[test]
    fn truncates_on_character_boundaries() {
        assert_eq!(bounded_text("a🦀b", 2), "a🦀");
    }

    #[test]
    fn table_cells_replace_control_separators() {
        assert_eq!(table_cell("a\tb\nc", 10), "a b c");
    }
}
