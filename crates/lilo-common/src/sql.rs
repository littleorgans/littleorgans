/// Tracks whether a dynamic SQL predicate list has already emitted `WHERE`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WhereClause {
    has_predicate: bool,
}

impl WhereClause {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            has_predicate: false,
        }
    }

    pub fn predicate_prefix(&mut self) -> &'static str {
        if self.has_predicate {
            " AND "
        } else {
            self.has_predicate = true;
            " WHERE "
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WhereClause;

    #[test]
    fn predicate_prefix_starts_with_where_then_and() {
        let mut clause = WhereClause::new();

        assert_eq!(clause.predicate_prefix(), " WHERE ");
        assert_eq!(clause.predicate_prefix(), " AND ");
        assert_eq!(clause.predicate_prefix(), " AND ");
    }
}
