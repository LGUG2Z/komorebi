// Golden-JSON generator for a `#[serde(tag="type", content="content")]` enum with a JsonSchema.
// Included by #[path] into each harness binary (and, after step 2, into one over komorebi-protocol).
//
//   golden_gen::run::<SocketMessage>("kde", outdir, &[("ActiveWindowBorder", "Border")], &["ApplyState"])
//   (aliases: serde aliases not in the schema; stubbed: variants whose payload type is a harness stub)
//
// 1. schema = schemars::schema_for!(T)  -> outdir/schema.json
// 2. for each oneOf branch: name = properties.type.const; content = minimal value for properties.content
// 3. instance {"type":name[,"content":..]} is deserialised into T and serialised again (the golden).
// 4. outdir/variants.json: { name: { instance, golden, roundtrip_equal, deserialised } } sorted by name.
//    outdir/variants.txt: the sorted names.  A summary goes to stdout.
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

fn resolve<'a>(root: &'a Value, r: &str) -> &'a Value {
    let path = r.strip_prefix("#/").unwrap_or_else(|| panic!("unsupported $ref {r}"));
    let mut cur = root;
    for seg in path.split('/') {
        cur = cur.get(seg).unwrap_or_else(|| panic!("dangling $ref {r}"));
    }
    cur
}

thread_local! {
    /// (branch index to use at every oneOf/anyOf/enum, prefer a null branch)
    static MODE: std::cell::Cell<(usize, bool)> = const { std::cell::Cell::new((0, false)) };
}

/// The smallest value the schema accepts; deterministic (branch MODE.0, lowest bound).
pub fn minimal(s: &Value, root: &Value, depth: usize) -> Value {
    assert!(depth < 40, "schema recursion too deep");
    let Some(o) = s.as_object() else { return Value::Null };
    if let Some(r) = o.get("$ref").and_then(Value::as_str) {
        let mut base = minimal(resolve(root, r), root, depth + 1);
        // siblings of $ref (schemars adds `properties` for an internally tagged variant)
        if let (Value::Object(m), Some(p)) = (&mut base, o.get("properties").and_then(Value::as_object)) {
            let req: Vec<&str> = o.get("required").and_then(Value::as_array).map(|r| r.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            for (k, v) in p {
                if req.contains(&k.as_str()) {
                    m.insert(k.clone(), minimal(v, root, depth + 1));
                }
            }
        }
        return base;
    }
    if let Some(c) = o.get("const") {
        return c.clone();
    }
    if let Some(e) = o.get("enum").and_then(Value::as_array) {
        let (k, _) = MODE.get();
        return e[k.min(e.len() - 1)].clone();
    }
    for k in ["oneOf", "anyOf"] {
        if let Some(b) = o.get(k).and_then(Value::as_array) {
            // prefer a branch that is not the bare null type; keep source order otherwise
            let (k, nulls) = MODE.get();
            let non_null: Vec<&Value> = b.iter().filter(|x| x.get("type") != Some(&json!("null"))).collect();
            let null = b.iter().find(|x| x.get("type") == Some(&json!("null")));
            let pick = match (nulls, null) {
                (true, Some(n)) => n,
                _ if non_null.is_empty() => &b[0],
                _ => non_null[k.min(non_null.len() - 1)],
            };
            return minimal(pick, root, depth + 1);
        }
    }
    if let Some(a) = o.get("allOf").and_then(Value::as_array) {
        let mut merged: Option<Map<String, Value>> = None;
        for x in a {
            let v = minimal(x, root, depth + 1);
            match (&mut merged, v) {
                (None, Value::Object(m)) => merged = Some(m),
                (Some(m), Value::Object(n)) => m.extend(n),
                (None, v) => return v,
                _ => {}
            }
        }
        return merged.map(Value::Object).unwrap_or(Value::Null);
    }
    let ty = match o.get("type") {
        Some(Value::String(t)) => t.as_str().to_owned(),
        Some(Value::Array(ts)) => ts
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .unwrap_or("null")
            .to_owned(),
        _ => {
            if o.contains_key("properties") {
                "object".into()
            } else if o.contains_key("items") || o.contains_key("prefixItems") {
                "array".into()
            } else {
                "null".into()
            }
        }
    };
    match ty.as_str() {
        "null" => Value::Null,
        "boolean" => json!(false),
        "integer" | "number" => {
            let mut v = o.get("minimum").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
            if let Some(x) = o.get("exclusiveMinimum").and_then(Value::as_f64) {
                v = v.max(x + 1.0);
            }
            if ty == "integer" { json!(v as i64) } else { json!(v) }
        }
        "string" => json!("x"),
        "array" => {
            if let Some(p) = o.get("prefixItems").and_then(Value::as_array) {
                return Value::Array(p.iter().map(|x| minimal(x, root, depth + 1)).collect());
            }
            let n = o.get("minItems").and_then(Value::as_u64).unwrap_or(0) as usize;
            let item = o.get("items").map(|i| minimal(i, root, depth + 1)).unwrap_or(Value::Null);
            Value::Array(vec![item; n])
        }
        "object" => {
            let mut m = Map::new();
            let req: Vec<&str> = o
                .get("required")
                .and_then(Value::as_array)
                .map(|r| r.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if let Some(p) = o.get("properties").and_then(Value::as_object) {
                for (k, v) in p {
                    if req.contains(&k.as_str()) {
                        m.insert(k.clone(), minimal(v, root, depth + 1));
                    }
                }
            }
            Value::Object(m)
        }
        t => panic!("unsupported type {t}"),
    }
}

pub fn run<T>(platform: &str, outdir: &Path, aliases: &[(&str, &str)], stubbed: &[&str])
where
    T: Serialize + DeserializeOwned + schemars::JsonSchema,
{
    std::fs::create_dir_all(outdir).unwrap();
    // paths expand environment variables (and ~ on macOS): pin them so the golden is host independent
    unsafe {
        std::env::set_var("HOME", "/golden-home");
        std::env::set_var("USERPROFILE", "C:\\golden-profile");
        std::env::set_var("GOLDEN_VAR", "golden-value");
    }
    let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
    std::fs::write(outdir.join("schema.json"), serde_json::to_string_pretty(&schema).unwrap() + "\n").unwrap();

    let branches = schema["oneOf"].as_array().expect("top-level oneOf");
    let mut out: BTreeMap<String, Value> = BTreeMap::new();
    let mut inst_by_name: BTreeMap<String, Value> = BTreeMap::new();
    for b in branches {
        let name = b["properties"]["type"]["const"].as_str().expect("type const").to_owned();
        let mut inst = Map::new();
        inst.insert("type".into(), json!(name));
        if let Some(c) = b["properties"].get("content") {
            inst.insert("content".into(), minimal(c, &schema, 0));
        }
        assert!(inst_by_name.insert(name, Value::Object(inst)).is_none(), "duplicate variant");
    }
    // more instances: other oneOf/enum branches (@1, @2, @3, only if different), null options (@nulls)
    let base: Vec<(String, Value)> = inst_by_name.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for b in branches {
        let name = b["properties"]["type"]["const"].as_str().unwrap().to_owned();
        let Some(c) = b["properties"].get("content") else { continue };
        let mut seen = vec![inst_by_name[&name]["content"].clone()];
        for (suffix, mode) in [("@1", (1, false)), ("@2", (2, false)), ("@3", (3, false)), ("@nulls", (0, true))] {
            MODE.set(mode);
            let v = minimal(c, &schema, 0);
            MODE.set((0, false));
            if !seen.contains(&v) {
                seen.push(v.clone());
                let inst = json!({ "type": name, "content": v });
                // the generator is naive (e.g. untagged unions): keep only instances the type accepts
                if serde_json::from_value::<T>(inst.clone()).is_ok() {
                    inst_by_name.insert(format!("{name}{suffix}"), inst);
                } else {
                    println!("  skipped alternative instance {name}{suffix} (not accepted by the type)");
                }
            }
        }
    }
    let _ = base;
    for (name, content) in [
        ("Load@~", "~/k.json"), ("Load@dollar", "$HOME/k.json"), ("Load@braces", "${GOLDEN_VAR}/k.json"),
        ("Load@percent", "%USERPROFILE%/k.json"), ("Load@psenv", "$Env:USERPROFILE/k.json"),
        ("Save@~", "~/k.json"), ("ReplaceConfiguration@dollar", "$HOME/k.json"),
        ("ReloadStaticConfiguration@percent", "%USERPROFILE%/k.json"),
    ] {
        let t = name.split('@').next().unwrap();
        if inst_by_name.contains_key(t) {
            inst_by_name.insert(name.to_owned(), json!({ "type": t, "content": content }));
        }
    }
    for (alias, target) in aliases {
        let mut v = inst_by_name[*target].clone();
        v["type"] = json!(alias);
        inst_by_name.insert((*alias).to_owned(), v);
    }
    let (mut de_ok, mut eq_ok) = (0usize, 0usize);
    for (name, inst) in &inst_by_name {
        let r = serde_json::from_value::<T>(inst.clone());
        let mut wire = Value::Null;
        let (golden, de, eq) = match r {
            Ok(t) => {
                let g = serde_json::to_value(&t).unwrap();
                // and the text form the socket really carries must read back
                let s = serde_json::to_string(&t).unwrap();
                wire = json!(s.clone());
                let back: T = serde_json::from_str(&s).expect("reserialised text must parse");
                assert_eq!(serde_json::to_value(&back).unwrap(), g, "{name}: unstable round trip");
                let eq = &g == inst;
                (g, true, eq)
            }
            Err(e) => (json!({ "error": e.to_string() }), false, false),
        };
        de_ok += de as usize;
        eq_ok += eq as usize;
        out.insert(
            name.clone(),
            json!({ "instance": inst, "wire": wire, "golden": golden, "deserialised": de, "roundtrip_equal": eq, "payload_stubbed": stubbed.contains(&name.as_str()) }),
        );
    }
    std::fs::write(outdir.join("variants.json"), serde_json::to_string_pretty(&out).unwrap() + "\n").unwrap();
    let names: Vec<&String> = out.keys().collect();
    std::fs::write(
        outdir.join("variants.txt"),
        names.iter().map(|n| format!("{n}\n")).collect::<String>(),
    )
    .unwrap();
    println!(
        "{platform}: schema branches {}, aliases {}, instances {}, deserialised {de_ok}, golden==instance {eq_ok}, payload stubbed {}",
        branches.len(),
        aliases.len(),
        out.len(),
        stubbed.len()
    );
    for (n, v) in &out {
        if v["deserialised"] == json!(false) || v["roundtrip_equal"] == json!(false) {
            println!("  note {n}: deserialised={} equal={} golden={}", v["deserialised"], v["roundtrip_equal"], v["golden"]);
        }
    }
}
