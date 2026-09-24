//! Structured-predicate compiler: translates the closed
//! `cronus_contract::FieldPredicate` vocabulary into a parameterized SQL
//! `WHERE` fragment over `memories` columns. SQLite expresses every
//! combinator in the vocabulary natively (arbitrary AND/OR/NOT nesting), so
//! the spec's post-fetch fallback for an inexpressible combinator is never
//! exercised by this backend — a property of SQLite, not a gap here.

use cronus_contract::{FieldPredicate, PredicateField, PredicateValue};
use rusqlite::types::ToSql;

fn column(field: PredicateField) -> &'static str {
    match field {
        PredicateField::Kind => "kind",
        PredicateField::Source => "source",
        PredicateField::WorkspaceId => "workspace_id",
        PredicateField::TrustScore => "trust_score",
        PredicateField::Confidence => "confidence",
        PredicateField::CreatedAt => "created_at",
        PredicateField::ValidAt => "valid_at",
        PredicateField::ExperienceOutcome => "experience_outcome",
    }
}

fn value_to_sql(value: &PredicateValue) -> Box<dyn ToSql> {
    match value {
        PredicateValue::Text(s) => Box::new(s.clone()),
        PredicateValue::Number(n) => Box::new(*n),
    }
}

/// Compile `predicate` into a `WHERE`-ready SQL fragment (no leading
/// `WHERE`), appending its bind values to `binds` in the same left-to-right
/// order the `?` placeholders appear.
pub(crate) fn compile(predicate: &FieldPredicate, binds: &mut Vec<Box<dyn ToSql>>) -> String {
    match predicate {
        FieldPredicate::Eq(f, v) => {
            binds.push(value_to_sql(v));
            format!("{} = ?", column(*f))
        }
        FieldPredicate::Ne(f, v) => {
            binds.push(value_to_sql(v));
            format!("{} != ?", column(*f))
        }
        FieldPredicate::Gt(f, v) => {
            binds.push(value_to_sql(v));
            format!("{} > ?", column(*f))
        }
        FieldPredicate::Ge(f, v) => {
            binds.push(value_to_sql(v));
            format!("{} >= ?", column(*f))
        }
        FieldPredicate::Lt(f, v) => {
            binds.push(value_to_sql(v));
            format!("{} < ?", column(*f))
        }
        FieldPredicate::Le(f, v) => {
            binds.push(value_to_sql(v));
            format!("{} <= ?", column(*f))
        }
        FieldPredicate::In(f, vs) => {
            let placeholders = vec!["?"; vs.len()].join(",");
            for v in vs {
                binds.push(value_to_sql(v));
            }
            format!("{} IN ({placeholders})", column(*f))
        }
        FieldPredicate::NotIn(f, vs) => {
            let placeholders = vec!["?"; vs.len()].join(",");
            for v in vs {
                binds.push(value_to_sql(v));
            }
            format!("{} NOT IN ({placeholders})", column(*f))
        }
        FieldPredicate::Contains(f, s) => {
            binds.push(Box::new(format!("%{s}%")));
            format!("{} LIKE ?", column(*f))
        }
        FieldPredicate::ContainsCi(f, s) => {
            binds.push(Box::new(format!("%{}%", s.to_lowercase())));
            format!("LOWER({}) LIKE ?", column(*f))
        }
        FieldPredicate::And(ps) => {
            let parts: Vec<String> = ps.iter().map(|p| compile(p, binds)).collect();
            format!("({})", parts.join(" AND "))
        }
        FieldPredicate::Or(ps) => {
            let parts: Vec<String> = ps.iter().map(|p| compile(p, binds)).collect();
            format!("({})", parts.join(" OR "))
        }
        FieldPredicate::Not(p) => format!("NOT ({})", compile(p, binds)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_compiles_to_a_single_placeholder() {
        let mut binds: Vec<Box<dyn ToSql>> = Vec::new();
        let sql = compile(
            &FieldPredicate::Eq(
                PredicateField::Kind,
                PredicateValue::Text("Convention".into()),
            ),
            &mut binds,
        );
        assert_eq!(sql, "kind = ?");
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn in_compiles_to_n_placeholders() {
        let mut binds: Vec<Box<dyn ToSql>> = Vec::new();
        let sql = compile(
            &FieldPredicate::In(
                PredicateField::Kind,
                vec![
                    PredicateValue::Text("Convention".into()),
                    PredicateValue::Text("KnownIssue".into()),
                ],
            ),
            &mut binds,
        );
        assert_eq!(sql, "kind IN (?,?)");
        assert_eq!(binds.len(), 2);
    }

    #[test]
    fn and_or_not_nest_with_parentheses() {
        let mut binds: Vec<Box<dyn ToSql>> = Vec::new();
        let pred = FieldPredicate::And(vec![
            FieldPredicate::Ge(PredicateField::TrustScore, PredicateValue::Number(0.5)),
            FieldPredicate::Not(Box::new(FieldPredicate::Eq(
                PredicateField::Source,
                PredicateValue::Text("Import".into()),
            ))),
        ]);
        let sql = compile(&pred, &mut binds);
        assert_eq!(sql, "(trust_score >= ? AND NOT (source = ?))");
        assert_eq!(binds.len(), 2);
    }
}
