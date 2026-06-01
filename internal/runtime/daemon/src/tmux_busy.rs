use lilo_rm_core::{RuntimeKind, strip_ansi_escapes};

const BUSY_INTERRUPT_MARKER: &str = "esc to interrupt";
const BOTTOM_STATUS_ROWS: usize = 3;

/// Claude Code busy footers are UI text, not an API. Keep this isolated so the
/// future transport or shim turn signal can replace the scrape in one place.
pub(crate) const CLAUDE_SPINNER_GLYPHS: &[char] = &['✻', '✽', '✶', '✳', '✢', '✤', '✥', '✦', '✧'];

pub(crate) fn agent_is_busy(runtime: &RuntimeKind, content: &str) -> bool {
    bottom_visible_rows(content)
        .iter()
        .any(|row| runtime_row_is_busy(runtime, row))
}

fn runtime_row_is_busy(runtime: &RuntimeKind, row: &str) -> bool {
    match runtime {
        RuntimeKind::Codex => has_interrupt_marker(row),
        RuntimeKind::Claude => has_interrupt_marker(row) || is_claude_spinner_row(row),
        RuntimeKind::Other(_) => {
            // Unknown runtimes default to delivery unless they expose the
            // shared interrupt marker. This avoids a 120 second wait on panes
            // whose UI contract is not known yet.
            has_interrupt_marker(row)
        }
    }
}

fn bottom_visible_rows(content: &str) -> Vec<String> {
    let rows = content
        .lines()
        .map(strip_ansi_escapes)
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    rows[rows.len().saturating_sub(BOTTOM_STATUS_ROWS)..].to_vec()
}

fn has_interrupt_marker(row: &str) -> bool {
    row.to_ascii_lowercase().contains(BUSY_INTERRUPT_MARKER)
}

fn is_claude_spinner_row(row: &str) -> bool {
    let trimmed = row.trim_start();
    let Some(glyph) = trimmed.chars().next() else {
        return false;
    };
    if !CLAUDE_SPINNER_GLYPHS.contains(&glyph) {
        return false;
    }
    let Some((_, after_ellipsis)) = trimmed.split_once("… (") else {
        return false;
    };
    starts_with_elapsed_timer(after_ellipsis)
}

fn starts_with_elapsed_timer(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|value| value.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_busy_marker_on_bottom_row_is_busy() {
        assert!(agent_is_busy(
            &RuntimeKind::Codex,
            "prompt\n> working esc to interrupt\n"
        ));
    }

    #[test]
    fn claude_busy_spinner_fixture_is_busy() {
        assert!(agent_is_busy(
            &RuntimeKind::Claude,
            "✳ Caramelizing… (52s · ↓ 3.2k tokens · thought for 2s)\n"
        ));
    }

    #[test]
    fn claude_non_default_spinner_fixture_is_busy() {
        assert!(agent_is_busy(
            &RuntimeKind::Claude,
            "✻ Contemplating… (9s · ↑ 1.1k tokens)\n"
        ));
    }

    #[test]
    fn claude_minute_plus_spinner_fixture_is_busy() {
        assert!(agent_is_busy(
            &RuntimeKind::Claude,
            "✻ Waddling… (3m 29s · ↓ 16.0k tokens)\n"
        ));
    }

    #[test]
    fn claude_non_timer_parenthetical_is_idle() {
        assert!(!agent_is_busy(
            &RuntimeKind::Claude,
            "✻ Loading… (loading provider state)\n"
        ));
    }

    #[test]
    fn claude_worked_and_churned_summaries_are_idle() {
        assert!(!agent_is_busy(
            &RuntimeKind::Claude,
            "✳ Worked for 52s · updated files\n"
        ));
        assert!(!agent_is_busy(
            &RuntimeKind::Claude,
            "✶ Churned for 8s · no changes\n"
        ));
    }

    #[test]
    fn old_scrollback_marker_is_ignored() {
        let content = [
            "old esc to interrupt",
            "line 1",
            "line 2",
            "line 3",
            "ready prompt",
        ]
        .join("\n");

        assert!(!agent_is_busy(&RuntimeKind::Codex, &content));
    }

    #[test]
    fn ansi_wrapped_status_rows_are_classified() {
        assert!(agent_is_busy(
            &RuntimeKind::Claude,
            "\u{1b}[35m✶ Distilling… (3s · ↓ 42 tokens)\u{1b}[0m\n"
        ));
        assert!(agent_is_busy(
            &RuntimeKind::Codex,
            "\u{1b}[2mEsc to interrupt\u{1b}[0m\n"
        ));
    }

    #[test]
    fn other_runtime_defaults_idle_without_marker() {
        assert!(!agent_is_busy(
            &RuntimeKind::Other("custom".to_owned()),
            "custom runtime ready\n"
        ));
    }

    #[test]
    fn other_runtime_uses_generic_interrupt_marker() {
        assert!(agent_is_busy(
            &RuntimeKind::Other("custom".to_owned()),
            "custom busy, esc to interrupt\n"
        ));
    }
}
