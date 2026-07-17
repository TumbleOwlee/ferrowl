//! Compile-time codegen for the bundled Lua script-template library (SC-R-036).
//!
//! Walks `templates/<context>/<name>.lua` and emits the `TEMPLATES` array into `OUT_DIR`, so adding
//! a template is just dropping a `.lua` in the right context directory — no edit to a Rust table.
//! Each file carries its one-line description on a leading `-- description: …` header comment; the
//! context is its parent directory and the name its file stem. The description header is stripped
//! from the embedded `code` so an inserted template's body is exactly the file minus that one line.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// (`directory name`, `ScriptContext` variant, sort rank). The rank fixes the order contexts appear
/// in the generated array; templates within a context are ordered by name.
const CONTEXTS: &[(&str, &str, u8)] = &[
    ("modbus", "TemplateContext::Modbus", 0),
    ("ocpp/client", "TemplateContext::OcppClient", 1),
    ("ocpp/server", "TemplateContext::OcppServer", 2),
    ("session", "TemplateContext::Session", 3),
];

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let templates = Path::new(&manifest).join("templates");
    println!("cargo:rerun-if-changed={}", templates.display());

    // Keyed by (context rank, name) so the emitted array is deterministic regardless of readdir
    // order: context groups in CONTEXTS order, names alphabetical within a group.
    let mut entries: BTreeMap<(u8, String), String> = BTreeMap::new();

    for (dir, ctx_variant, rank) in CONTEXTS {
        let ctx_dir = templates.join(dir);
        println!("cargo:rerun-if-changed={}", ctx_dir.display());
        let read = match fs::read_dir(&ctx_dir) {
            Ok(r) => r,
            Err(e) => panic!("template context dir {}: {e}", ctx_dir.display()),
        };
        for dirent in read {
            let path = dirent.expect("read template dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("lua") {
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_else(|| panic!("non-UTF-8 template name {}", path.display()))
                .to_string();
            let raw = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let (description, code) = split_header(&raw).unwrap_or_else(|| {
                panic!("{} lacks a `-- description:` header line", path.display())
            });

            let entry = format!(
                "    ScriptTemplate {{\n        name: {name:?},\n        description: {description:?},\n        contexts: &[{ctx_variant}],\n        code: {code:?},\n    }},\n"
            );
            entries.insert((*rank, name), entry);
        }
    }

    let mut out = String::from("pub static TEMPLATES: &[ScriptTemplate] = &[\n");
    for entry in entries.values() {
        out.push_str(entry);
    }
    out.push_str("];\n");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR set by cargo");
    let dest = Path::new(&out_dir).join("templates_generated.rs");
    fs::write(&dest, out).unwrap_or_else(|e| panic!("write {}: {e}", dest.display()));
}

/// Split a template file into its `-- description:` header text and the remaining code body (the
/// file with exactly that one header line removed). Returns `None` if no header line is present.
fn split_header(raw: &str) -> Option<(String, String)> {
    let mut description = None;
    let mut body = String::new();
    for line in raw.lines() {
        if description.is_none()
            && let Some(rest) = line.strip_prefix("-- description:")
        {
            description = Some(rest.trim().to_string());
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    description.map(|d| (d, body))
}
