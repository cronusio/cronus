//! Shared helpers for integration tests that run workflows they did not write
//! (the normative fixture corpus): input that satisfies whatever `@in:`
//! contract the workflow declares, so the run reaches the code under test.

#![allow(dead_code)]

use nodus::{executor::Value, parser::Parser};

/// A value of the right kind for a declared `@in:` type.
fn sample_value(type_name: &str) -> Value {
    match type_name {
        "int" => Value::Int(1),
        "float" => Value::Float(1.0),
        "bool" => Value::Bool(true),
        // Empty: the fixtures iterate `$in.items`, and iterating is not what
        // the tests that use this helper are about (they ran with no input
        // before the input contract existed, i.e. zero iterations).
        "list" => Value::List(Vec::new()),
        "obj" | "map" => Value::Map(Vec::new()),
        "null" => Value::Null,
        _ => Value::Text("sample".to_string()),
    }
}

/// An input map supplying every field `source` declares in `@in:`.
pub fn sample_input(source: &str) -> Value {
    let ast = Parser::parse(source).expect("test workflow parses");
    let fields = ast
        .input_decl
        .map(|decl| decl.fields)
        .unwrap_or_default()
        .into_iter()
        .map(|field| {
            (
                field.name.trim_start_matches('$').to_string(),
                sample_value(&field.type_name),
            )
        })
        .collect();
    Value::Map(fields)
}
