//! Integration tests for JSON Schema Test Suite compatibility
//!
//! This module tests that schemars-docgen can generate HTML documentation
//! for all schemas in the JSON-Schema-Test-Suite draft2020-12 examples
//! without crashing.

use chrono::Local;
use chrono::Utc;
use rayon::prelude::*;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tera::Context;
use tera::Tera;

/// Represents a test suite from the JSON Schema Test Suite
#[derive(Debug, Deserialize)]
struct TestSuite {
    description: String,
    schema: serde_json::Value,
    #[serde(default)]
    #[allow(dead_code)]
    tests: Vec<serde_json::Value>,
}

/// A schema test case with all context needed for parallel execution
#[derive(Debug)]
struct SchemaTestCase {
    #[allow(dead_code)]
    file_path: PathBuf,
    relative_path: String,
    index: usize,
    description: String,
    schema: serde_json::Value,
    output_path: PathBuf,
}

/// Get the project root directory
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Get the fixtures directory
fn fixtures_dir() -> PathBuf {
    project_root().join("tests/fixtures/json-schema-test-suite/draft2020-12")
}

/// Get the test output directory
fn output_dir() -> PathBuf {
    project_root().join("tests/outputs/json-schema-test-suite/draft2020-12")
}

/// Extract all schemas from a test suite file
fn extract_schemas(file_path: &Path) -> Vec<(String, serde_json::Value)> {
    let content = fs::read_to_string(file_path).expect("Failed to read test file");
    let suites: Vec<TestSuite> = serde_json::from_str(&content).expect("Failed to parse test file");

    suites
        .into_iter()
        .map(|suite| (suite.description, suite.schema))
        .collect()
}

/// Run the CLI against a schema and return success/failure
fn run_cli(schema: &serde_json::Value, output_path: &Path) -> Result<(), String> {
    // Create a unique temporary file for the schema (use thread id to avoid conflicts)
    let temp_dir = std::env::temp_dir();
    let schema_file = temp_dir.join(format!(
        "test_schema_{:?}_{}.json",
        std::thread::current().id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    fs::write(&schema_file, serde_json::to_string_pretty(schema).unwrap())
        .map_err(|e| format!("Failed to write schema file: {}", e))?;

    // Ensure output directory exists
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create output directory: {}", e))?;
    }

    // Run the CLI
    let output = Command::new(env!("CARGO_BIN_EXE_schemars-docgen"))
        .arg(&schema_file)
        .arg("-o")
        .arg(output_path)
        .output()
        .map_err(|e| format!("Failed to execute CLI: {}", e))?;

    // Clean up temp file
    let _ = fs::remove_file(&schema_file);

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!(
            "CLI failed with status {:?}\nstderr: {}\nstdout: {}",
            output.status.code(),
            stderr,
            stdout
        ))
    }
}

/// Collect all JSON test files from a directory recursively
fn collect_test_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if dir.is_dir() {
        for entry in fs::read_dir(dir).expect("Failed to read directory") {
            let entry = entry.expect("Failed to read entry");
            let path = entry.path();

            if path.is_dir() {
                files.extend(collect_test_files(&path));
            } else if path.extension().map_or(false, |ext| ext == "json") {
                files.push(path);
            }
        }
    }

    files.sort();
    files
}

/// Convert a fixture file path to its corresponding output directory
fn fixture_to_output_dir(fixture_path: &Path) -> PathBuf {
    let fixtures_base = fixtures_dir();
    let relative = fixture_path
        .strip_prefix(&fixtures_base)
        .expect("Fixture path should be under fixtures dir");

    // Remove .json extension and use as directory name
    let stem = relative.with_extension("");
    output_dir().join(stem)
}

/// Collect all test cases from all fixture files
fn collect_all_test_cases() -> Vec<SchemaTestCase> {
    let fixtures = fixtures_dir();
    let test_files = collect_test_files(&fixtures);

    test_files
        .into_iter()
        .flat_map(|file_path| {
            let schemas = extract_schemas(&file_path);
            let output_base = fixture_to_output_dir(&file_path);
            let relative_path = file_path
                .strip_prefix(&fixtures)
                .unwrap_or(&file_path)
                .display()
                .to_string();

            schemas
                .into_iter()
                .enumerate()
                .map(move |(idx, (description, schema))| SchemaTestCase {
                    file_path: file_path.clone(),
                    relative_path: relative_path.clone(),
                    index: idx,
                    description,
                    schema,
                    output_path: output_base.join(format!("{}.html", idx)),
                })
        })
        .collect()
}

/// Data structure for a schema entry in the index template
#[derive(Debug, Serialize)]
struct IndexSchema {
    index: usize,
    description: String,
    output_path: String,
    has_content: bool,
    features: Vec<String>,
}

/// Data structure for a file group in the index template
#[derive(Debug, Serialize)]
struct IndexGroup {
    file_path: String,
    schemas: Vec<IndexSchema>,
}

/// Analyze a schema and return (has_content, features)
/// has_content is true if the schema has properties or $defs (will show Configuration/Types)
/// features lists the JSON Schema keywords present in the schema
fn analyze_schema(schema: &serde_json::Value) -> (bool, Vec<String>) {
    let mut features = Vec::new();

    let obj = match schema.as_object() {
        Some(o) => o,
        None => {
            // Handle boolean schemas
            if schema.as_bool() == Some(true) {
                return (false, vec!["boolean: true".to_string()]);
            } else if schema.as_bool() == Some(false) {
                return (false, vec!["boolean: false".to_string()]);
            }
            return (false, vec!["non-object".to_string()]);
        }
    };

    if obj.is_empty() {
        return (false, vec!["empty schema".to_string()]);
    }

    let has_properties = obj.contains_key("properties");
    let has_defs = obj.contains_key("$defs") || obj.contains_key("definitions");

    // Collect features
    if has_properties {
        features.push("properties".to_string());
    }
    if has_defs {
        features.push("$defs".to_string());
    }
    if obj.contains_key("$ref") {
        features.push("$ref".to_string());
    }
    if obj.contains_key("type") {
        if let Some(t) = obj.get("type") {
            if let Some(s) = t.as_str() {
                features.push(format!("type: {}", s));
            } else if let Some(arr) = t.as_array() {
                let types: Vec<_> = arr.iter().filter_map(|v| v.as_str()).collect();
                features.push(format!("type: [{}]", types.join(", ")));
            }
        }
    }
    if obj.contains_key("items") {
        features.push("items".to_string());
    }
    if obj.contains_key("prefixItems") {
        features.push("prefixItems".to_string());
    }
    if obj.contains_key("additionalProperties") {
        features.push("additionalProperties".to_string());
    }
    if obj.contains_key("patternProperties") {
        features.push("patternProperties".to_string());
    }
    if obj.contains_key("allOf") {
        features.push("allOf".to_string());
    }
    if obj.contains_key("anyOf") {
        features.push("anyOf".to_string());
    }
    if obj.contains_key("oneOf") {
        features.push("oneOf".to_string());
    }
    if obj.contains_key("not") {
        features.push("not".to_string());
    }
    if obj.contains_key("if") {
        features.push("if/then/else".to_string());
    }
    if obj.contains_key("const") {
        features.push("const".to_string());
    }
    if obj.contains_key("enum") {
        features.push("enum".to_string());
    }
    if obj.contains_key("$dynamicRef") {
        features.push("$dynamicRef".to_string());
    }
    if obj.contains_key("$dynamicAnchor") {
        features.push("$dynamicAnchor".to_string());
    }
    if obj.contains_key("contains") {
        features.push("contains".to_string());
    }
    if obj.contains_key("dependentSchemas") {
        features.push("dependentSchemas".to_string());
    }
    if obj.contains_key("dependentRequired") {
        features.push("dependentRequired".to_string());
    }
    if obj.contains_key("unevaluatedItems") {
        features.push("unevaluatedItems".to_string());
    }
    if obj.contains_key("unevaluatedProperties") {
        features.push("unevaluatedProperties".to_string());
    }

    // If no features found, list what keys are present
    if features.is_empty() {
        let keys: Vec<_> = obj.keys().take(3).cloned().collect();
        features.push(format!("other: {}", keys.join(", ")));
    }

    (has_properties || has_defs, features)
}

/// Get git commit hash
fn get_git_commit_hash() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Check if git repo is dirty
fn is_git_dirty() -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Generate an index.html file with links to all test outputs using Tera
fn generate_index_html(test_cases: &[SchemaTestCase]) {
    let output_base = project_root().join("tests/outputs");
    let index_path = output_base.join("index.html");

    // Group test cases by their source file (relative_path)
    let mut grouped: BTreeMap<String, Vec<&SchemaTestCase>> = BTreeMap::new();
    for test_case in test_cases {
        grouped
            .entry(test_case.relative_path.clone())
            .or_default()
            .push(test_case);
    }

    // Convert to template-friendly structure
    let mut with_content = 0;
    let mut without_content = 0;

    let groups: Vec<IndexGroup> = grouped
        .into_iter()
        .map(|(file_path, cases)| {
            let schemas = cases
                .into_iter()
                .map(|case| {
                    let relative_output = case
                        .output_path
                        .strip_prefix(&output_base)
                        .expect("Output path should be under output base");
                    let (has_content, features) = analyze_schema(&case.schema);
                    if has_content {
                        with_content += 1;
                    } else {
                        without_content += 1;
                    }
                    IndexSchema {
                        index: case.index,
                        description: case.description.clone(),
                        output_path: relative_output.display().to_string(),
                        has_content,
                        features,
                    }
                })
                .collect();
            IndexGroup { file_path, schemas }
        })
        .collect();

    // Load and render template
    let template_path = project_root().join("src/templates/test_index.html");
    let template_content =
        fs::read_to_string(&template_path).expect("Failed to read test_index.html template");

    let mut tera = Tera::default();
    tera.add_raw_template("test_index.html", &template_content)
        .expect("Failed to parse template");

    // Get git and time info
    let commit_hash = get_git_commit_hash();
    let is_dirty = is_git_dirty();
    let time_utc = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let time_local = Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string();

    let mut context = Context::new();
    context.insert("total_schemas", &test_cases.len());
    context.insert("total_files", &groups.len());
    context.insert("with_content", &with_content);
    context.insert("without_content", &without_content);
    context.insert("commit_hash", &commit_hash);
    context.insert("is_dirty", &is_dirty);
    context.insert("time_utc", &time_utc);
    context.insert("time_local", &time_local);
    context.insert("groups", &groups);

    let html = tera
        .render("test_index.html", &context)
        .expect("Failed to render template");

    fs::write(&index_path, html).expect("Failed to write index.html");
    println!("Generated index.html at {:?}", index_path);
}

#[test]
fn test_all_json_schema_test_suite_schemas() {
    let fixtures = fixtures_dir();
    if !fixtures.exists() {
        panic!(
            "Fixtures directory not found: {:?}\n\
            Please run: git clone --depth 1 https://github.com/json-schema-org/JSON-Schema-Test-Suite.git /tmp/json-schema-test-suite && \
            mkdir -p fixtures/json-schema-test-suite && \
            cp -r /tmp/json-schema-test-suite/tests/draft2020-12 fixtures/json-schema-test-suite/ && \
            rm -rf /tmp/json-schema-test-suite",
            fixtures
        );
    }

    // Collect all test cases upfront
    let test_cases = collect_all_test_cases();
    let total_schemas = test_cases.len();
    println!("Found {} schemas to test", total_schemas);

    // Counters for progress
    let passed = AtomicUsize::new(0);
    let failed: Mutex<Vec<(String, usize, String, serde_json::Value, String)>> =
        Mutex::new(Vec::new());

    // Run tests in parallel
    test_cases.par_iter().for_each(|test_case| {
        match run_cli(&test_case.schema, &test_case.output_path) {
            Ok(()) => {
                passed.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                failed.lock().unwrap().push((
                    test_case.relative_path.clone(),
                    test_case.index,
                    test_case.description.clone(),
                    test_case.schema.clone(),
                    e,
                ));
            }
        }
    });

    let passed_count = passed.load(Ordering::Relaxed);
    let failed_list = failed.into_inner().unwrap();

    println!("\n=== Summary ===");
    println!("Total schemas: {}", total_schemas);
    println!("Passed: {}", passed_count);
    println!("Failed: {}", failed_list.len());

    if !failed_list.is_empty() {
        println!("\n=== Failed Schemas ===");
        for (relative_path, idx, desc, schema, error) in &failed_list {
            println!("\n--- {}[{}]: {} ---", relative_path, idx, desc);
            println!("Schema: {}", serde_json::to_string_pretty(schema).unwrap());
            println!("Error: {}", error);
        }
        panic!(
            "{} schemas failed to generate documentation",
            failed_list.len()
        );
    }

    // Generate index.html with links to all outputs
    generate_index_html(&test_cases);
}

/// Test a single schema file - useful for debugging specific failures
#[test]
#[ignore]
fn test_single_schema_file() {
    let file_path = fixtures_dir().join("type.json");
    let schemas = extract_schemas(&file_path);
    let output_base = fixture_to_output_dir(&file_path);

    for (idx, (description, schema)) in schemas.iter().enumerate() {
        let output_path = output_base.join(format!("{}.html", idx));
        println!("Testing: {} - {}", idx, description);
        println!("Schema: {}", serde_json::to_string_pretty(schema).unwrap());

        match run_cli(schema, &output_path) {
            Ok(()) => println!("  PASS"),
            Err(e) => panic!("  FAIL: {}", e),
        }
    }
}
