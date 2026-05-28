#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownMigration {
    pub version: i64,
    pub description: String,
}

pub fn known_migrations() -> Vec<KnownMigration> {
    vec![KnownMigration {
        version: 1,
        description: "unified schema".to_string(),
    }]
}
