use super::contract::SharedContent;

pub fn render_selector_grammar_block(shared: &SharedContent) -> Option<String> {
    let grammar = shared.selector_grammar.as_ref()?;
    let mut lines = Vec::new();
    lines.push("Grammar:".to_string());
    lines.extend(grammar.forms.iter().map(|form| format!("  {form}")));
    lines.push("Examples:".to_string());
    lines.extend(
        grammar
            .examples
            .iter()
            .map(|example| format!("  {example}")),
    );
    Some(lines.join("\n"))
}

pub fn rust_const_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}
