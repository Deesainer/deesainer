use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::Value;

const LAYERS: &[(&str, u8)] = &[
    ("deesainer-geom", 0),
    ("deesainer-color", 0),
    ("deesainer-figfile", 0),
    ("deesainer-doc", 1),
    ("deesainer-ops", 2),
    ("deesainer-vector", 2),
    ("deesainer-text", 2),
    ("deesainer-algo", 2),
    ("deesainer-render", 3),
    ("deesainer-format", 3),
    ("deesainer-tools", 4),
    ("deesainer-mesh", 4),
    ("deesainer-io", 4),
    ("deesainer-handoff", 4),
    ("deesainer-sync", 4),
    ("deesainer-plugins", 4),
    ("deesainer-engine", 5),
    ("deesainer-gpu", 5),
    ("deesainer-automation", 6),
    ("deesainer-server", 6),
    ("deesainer-ui", 6),
    ("deesainer-cli", 7),
    ("deesainer", 7),
    ("deesainer-relay", 7),
    ("deesainer-teamserver", 7),
    ("deesainer-web", 7),
];

const UI_LAYER: u8 = 6;
const UI_CRATES: &[&str] = &["egui", "eframe", "egui-winit", "egui-wgpu", "winit"];

const ASSET_DIRS: &[&str] = &["assets", "docs/images", "docs/brand"];
const ASSET_IGNORED: &[&str] = &[".gitkeep", ".DS_Store"];

type TaskResult = Result<(), String>;

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let result = workspace_root().and_then(|root| match task.as_str() {
        "layers" => layers(&root),
        "assets" => assets(&root),
        "wasm" => wasm(&root),
        "ci" => ci(&root),
        "parity" => parity(&root, std::env::args().any(|a| a == "--check")),
        "scorecard" => scorecard(&root),
        "version" => version(&root),
        "icons" => icons(&root),
        "licenses" => licenses(&root, std::env::args().any(|a| a == "--check")),
        _ => Err("usage: cargo xtask <layers | assets | wasm | parity [--check] | scorecard | version set X.Y.Z | ci>".to_owned()),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask {task}: {err}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "cannot locate the workspace root".to_owned())
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn run(root: &Path, args: &[&str]) -> TaskResult {
    println!("$ cargo {}", args.join(" "));
    let status = Command::new(cargo())
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`cargo {}` failed", args.join(" ")))
    }
}

fn layer_of(name: &str) -> Option<u8> {
    LAYERS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

fn layers(root: &Path) -> TaskResult {
    let output = Command::new(cargo())
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let meta: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("bad cargo metadata: {e}"))?;
    let packages = meta["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?;

    let mut violations = Vec::new();
    for package in packages {
        let name = package["name"].as_str().unwrap_or_default();
        if name == "xtask" {
            continue;
        }
        let Some(layer) = layer_of(name) else {
            violations.push(format!("{name} is not in the xtask LAYERS table"));
            continue;
        };
        for dep in package["dependencies"].as_array().into_iter().flatten() {
            let dep_name = dep["name"].as_str().unwrap_or_default();
            if dep["kind"].as_str() == Some("dev") {
                continue;
            }
            if let Some(dep_layer) = layer_of(dep_name) {
                if dep_layer >= layer {
                    violations.push(format!(
                        "{name} (L{layer}) depends on {dep_name} (L{dep_layer}): only lower layers allowed"
                    ));
                }
            } else if layer < UI_LAYER && UI_CRATES.contains(&dep_name) {
                violations.push(format!(
                    "{name} (L{layer}) depends on UI crate {dep_name}: only L{UI_LAYER}+ may"
                ));
            }
        }
    }
    if violations.is_empty() {
        println!("layers: ok ({} crates)", LAYERS.len());
        Ok(())
    } else {
        Err(format!(
            "layering violations:\n  {}",
            violations.join("\n  ")
        ))
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> TaskResult {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn assets(root: &Path) -> TaskResult {
    let mut files = Vec::new();
    for dir in ASSET_DIRS {
        let dir = root.join(dir);
        if dir.is_dir() {
            collect_files(&dir, &mut files)?;
        }
    }
    let attribution = std::fs::read_to_string(root.join("ATTRIBUTION.md")).unwrap_or_default();
    let mut missing = Vec::new();
    for file in &files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if ASSET_IGNORED.contains(&name) {
            continue;
        }
        let rel = file
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if !attribution.contains(&format!("`{rel}`")) {
            missing.push(rel);
        }
    }
    if missing.is_empty() {
        println!("assets: ok");
        Ok(())
    } else {
        Err(format!(
            "assets without an ATTRIBUTION.md row (add `path` with author, source, licence, use):\n  {}",
            missing.join("\n  ")
        ))
    }
}

fn wasm(root: &Path) -> TaskResult {
    let mut args = vec!["check", "--target", "wasm32-unknown-unknown"];
    for (name, layer) in LAYERS {
        if *layer < UI_LAYER {
            args.push("-p");
            args.push(name);
        }
    }
    args.extend(["-p", "deesainer-ui", "-p", "deesainer-web"]);
    run(root, &args)
}

fn ci(root: &Path) -> TaskResult {
    run(root, &["fmt", "--all", "--", "--check"])?;
    run(
        root,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(root, &["test", "--workspace"])?;
    layers(root)?;
    assets(root)?;
    wasm(root)?;
    parity(root, true)?;
    licenses(root, true)?;
    println!("ci: all gates green");
    Ok(())
}

struct Row {
    area: String,
    tier: String,
    status: String,
}

const START: &str = "<!-- parity-summary:start -->";
const END: &str = "<!-- parity-summary:end -->";

fn parity(root: &Path, check: bool) -> TaskResult {
    let path = root.join("docs/parity.md");
    if !path.exists() {
        println!("parity: no tracker, skipped");
        return Ok(());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let registry = deesainer_engine::CommandRegistry::builtin().map_err(|e| e.to_string())?;
    let mut rows: Vec<Row> = Vec::new();
    let mut errors = Vec::new();
    let mut area = String::new();
    let mut seen = std::collections::BTreeSet::new();
    for (n, line) in text.lines().enumerate() {
        if let Some(h) = line.strip_prefix("## ") {
            area = h.trim().to_owned();
            continue;
        }
        if !line.starts_with("| FG-") {
            continue;
        }
        let cols: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cols.len() < 6 {
            errors.push(format!("line {}: expected 6 columns", n + 1));
            continue;
        }
        let (id, tier, status, evidence) = (cols[0], cols[2], cols[3], cols[4]);
        if !seen.insert(id.to_owned()) {
            errors.push(format!("{id}: duplicate id"));
        }
        if !["P0", "P1", "P2", "OOS"].contains(&tier) {
            errors.push(format!("{id}: tier {tier:?} is not P0, P1, P2 or OOS"));
        }
        if !["done", "partial", "missing", "oos"].contains(&status) {
            errors.push(format!(
                "{id}: status {status:?} is not done, partial, missing or oos"
            ));
        }
        if status == "done" && evidence.is_empty() {
            errors.push(format!("{id}: done rows need evidence"));
        }
        for item in evidence.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(cmd) = item.strip_prefix("cmd:") {
                if registry.get(cmd).is_none() {
                    errors.push(format!("{id}: command {cmd} does not exist"));
                }
            } else if let Some(file) = item.strip_prefix("file:") {
                if !root.join(file).exists() {
                    errors.push(format!("{id}: file {file} does not exist"));
                }
            } else {
                errors.push(format!(
                    "{id}: evidence {item:?} must be cmd:<id> or file:<path>"
                ));
            }
        }
        rows.push(Row {
            area: area.clone(),
            tier: tier.to_owned(),
            status: status.to_owned(),
        });
    }
    if !errors.is_empty() {
        return Err(format!(
            "docs/parity.md has {} problem(s):\n  {}",
            errors.len(),
            errors.join("\n  ")
        ));
    }
    let summary = summary(&rows);
    let (Some(a), Some(b)) = (text.find(START), text.find(END)) else {
        return Err("docs/parity.md is missing the summary markers".into());
    };
    let updated = format!("{}{START}\n{summary}{}", &text[..a], &text[b..]);
    if updated != text {
        if check {
            return Err("the parity summary is out of date; run `cargo xtask parity`".into());
        }
        std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    }
    println!("parity: ok ({} rows)", rows.len());
    Ok(())
}

fn summary(rows: &[Row]) -> String {
    let mut out = String::from(
        "| Area | Done | Partial | Missing | OOS |\n| --- | ---: | ---: | ---: | ---: |\n",
    );
    let mut areas: Vec<&str> = Vec::new();
    for r in rows {
        if !areas.contains(&r.area.as_str()) {
            areas.push(&r.area);
        }
    }
    let count = |f: &dyn Fn(&Row) -> bool| rows.iter().filter(|r| f(r)).count();
    for a in areas {
        out.push_str(&format!(
            "| {a} | {} | {} | {} | {} |\n",
            count(&|r| r.area == a && r.status == "done"),
            count(&|r| r.area == a && r.status == "partial"),
            count(&|r| r.area == a && r.status == "missing"),
            count(&|r| r.area == a && r.status == "oos"),
        ));
    }
    out.push_str("\n| Tier | Rows | Done | Partial | Missing | Completion |\n| --- | ---: | ---: | ---: | ---: | ---: |\n");
    let mut weighted = 0.0;
    let mut total = 0.0;
    for (tier, weight) in [("P0", 3.0), ("P1", 2.0), ("P2", 1.0), ("OOS", 0.0)] {
        let n = count(&|r| r.tier == tier);
        let done = count(&|r| r.tier == tier && r.status == "done");
        let partial = count(&|r| r.tier == tier && r.status == "partial");
        let missing = count(&|r| r.tier == tier && r.status == "missing");
        let completion = if n == 0 || tier == "OOS" {
            "-".to_owned()
        } else {
            format!(
                "{:.0}%",
                (done as f64 + partial as f64 / 2.0) / n as f64 * 100.0
            )
        };
        out.push_str(&format!(
            "| {tier} | {n} | {done} | {partial} | {missing} | {completion} |\n"
        ));
        weighted += weight * (done as f64 + partial as f64 / 2.0);
        total += weight * n as f64;
    }
    let score = if total > 0.0 {
        weighted / total * 100.0
    } else {
        0.0
    };
    out.push_str(&format!(
        "\nWeighted score (P0 x3, P1 x2, P2 x1; partial counts half): **{score:.0}%**\n"
    ));
    out
}

fn scorecard(root: &Path) -> TaskResult {
    println!("$ cargo run --release -p deesainer-cli -- bench");
    let output = Command::new(cargo())
        .args([
            "run",
            "--quiet",
            "--release",
            "-p",
            "deesainer-cli",
            "--",
            "bench",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run the benchmark: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let report: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("bad benchmark output: {e}"))?;
    let mut md = String::from("# Scorecard\n\nMeasured by `cargo xtask scorecard` (release build). Numbers are dated and machine-specific; budgets are the targets from ROADMAP.md. A miss is a bug to fix or a budget to argue about in writing, never to hide.\n\n");
    md.push_str(&format!(
        "- Date: {}\n- Machine: {}\n- Commands registered: {}\n\n",
        report["date"].as_str().unwrap_or("?"),
        report["machine"].as_str().unwrap_or("?"),
        report["commands"]
    ));
    md.push_str("| Scenario | Measured | Budget | Result |\n| --- | ---: | ---: | --- |\n");
    let mut misses = 0;
    for s in report["scenarios"].as_array().into_iter().flatten() {
        let (value, budget) = (
            s["ms"].as_f64().unwrap_or(f64::NAN),
            s["budgetMs"].as_f64().unwrap_or(f64::NAN),
        );
        let ok = value <= budget;
        if !ok {
            misses += 1;
        }
        md.push_str(&format!(
            "| {} | {:.2} ms | {:.0} ms | {} |\n",
            s["name"].as_str().unwrap_or("?"),
            value,
            budget,
            if ok { "pass" } else { "**miss**" }
        ));
    }
    md.push_str(&format!(
        "\n{} of {} scenarios within budget. File size of the 10k-layer document: {} KB.\n",
        report["scenarios"].as_array().map_or(0, Vec::len) - misses,
        report["scenarios"].as_array().map_or(0, Vec::len),
        report["fileKb"]
    ));
    std::fs::write(root.join("docs/scorecard.md"), md).map_err(|e| e.to_string())?;
    println!("scorecard: written ({misses} budget miss(es))");
    Ok(())
}

fn version(root: &Path) -> TaskResult {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let [set, v] = args.as_slice() else {
        return Err("usage: cargo xtask version set X.Y.Z".into());
    };
    let (core, pre) = v.split_once('-').unwrap_or((v, ""));
    let pre_ok = pre.is_empty()
        || pre
            .split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()));
    if set != "set"
        || core.split('.').count() != 3
        || !core.split('.').all(|p| p.parse::<u32>().is_ok())
        || !pre_ok
    {
        return Err("usage: cargo xtask version set X.Y.Z[-pre.N]".into());
    }
    let path = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut done = false;
    let out: Vec<String> = text
        .lines()
        .map(|l| {
            if !done && l.starts_with("version = \"") {
                done = true;
                format!("version = \"{v}\"")
            } else {
                l.to_owned()
            }
        })
        .collect();
    if !done {
        return Err("no workspace version line found".into());
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, out.join("\n") + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    println!("version: {v}");
    Ok(())
}

fn icons(root: &Path) -> TaskResult {
    let dir = root.join("assets/app-icon");
    let svg = std::fs::read_to_string(dir.join("deesainer.svg")).map_err(|e| e.to_string())?;
    let imported = deesainer_io::import_svg(&svg, "icon").map_err(|e| e.to_string())?;
    let mut session = deesainer_engine::Session::new().map_err(|e| e.to_string())?;
    let reply = session
        .dispatch("import.svg", &serde_json::json!({ "data": svg }))
        .map_err(|e| e.to_string())?;
    let frame = deesainer_engine::doc::NodeId(reply["id"].as_u64().ok_or("no frame")?);
    let size = imported.size.0.max(1.0);
    std::fs::create_dir_all(dir.join("png")).map_err(|e| e.to_string())?;
    let mut pngs = Vec::new();
    for px in [16u32, 32, 64, 128, 256, 512, 1024] {
        let png = deesainer_io::export_png(session.doc(), frame, f64::from(px) / size)
            .map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("png/{px}.png")), &png).map_err(|e| e.to_string())?;
        pngs.push((px, png));
    }
    let ico_entries: Vec<&(u32, Vec<u8>)> = pngs.iter().filter(|(px, _)| *px <= 256).collect();
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(ico_entries.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * ico_entries.len() as u32;
    for (px, png) in &ico_entries {
        let dim = if *px >= 256 { 0u8 } else { *px as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &ico_entries {
        ico.extend_from_slice(png);
    }
    std::fs::write(dir.join("deesainer.ico"), ico).map_err(|e| e.to_string())?;
    let mut body = Vec::new();
    for (code, px) in [
        (b"ic07", 128u32),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
    ] {
        if let Some((_, png)) = pngs.iter().find(|(p, _)| *p == px) {
            body.extend_from_slice(code);
            body.extend_from_slice(&((png.len() + 8) as u32).to_be_bytes());
            body.extend_from_slice(png);
        }
    }
    let mut icns = b"icns".to_vec();
    icns.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    icns.extend_from_slice(&body);
    std::fs::write(dir.join("deesainer.icns"), icns).map_err(|e| e.to_string())?;
    println!("icons: written to {}", dir.display());
    Ok(())
}

const ALLOWED_LICENSES: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "0BSD",
    "CC0-1.0",
    "Unlicense",
    "BSL-1.0",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "MIT-0",
    "OFL-1.1",
    "Apache-2.0 WITH LLVM-exception",
    "BlueOak-1.0.0",
    "CDLA-Permissive-2.0",
];

fn license_ok(expr: &str) -> bool {
    expr.replace('/', " OR ").split(" OR ").any(|alt| {
        alt.trim_matches(|c| c == '(' || c == ')' || c == ' ')
            .split(" AND ")
            .all(|t| {
                ALLOWED_LICENSES.contains(&t.trim_matches(|c| c == '(' || c == ')' || c == ' '))
            })
    })
}

fn licenses(root: &Path, check: bool) -> TaskResult {
    const TARGETS: &[&str] = &[
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "wasm32-unknown-unknown",
    ];
    let mut rows: Vec<(String, String, String)> = Vec::new();
    let mut bad = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for target in TARGETS {
        let output = Command::new(cargo())
            .args([
                "metadata",
                "--format-version",
                "1",
                "--filter-platform",
                target,
            ])
            .current_dir(root)
            .output()
            .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        let meta: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
        let workspace: Vec<&str> = meta["workspace_members"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for p in meta["packages"].as_array().into_iter().flatten() {
            let id = p["id"].as_str().unwrap_or_default();
            if workspace.contains(&id) || seen.iter().any(|s| s == id) {
                continue;
            }
            seen.push(id.to_owned());
            let name = p["name"].as_str().unwrap_or_default().to_owned();
            let version = p["version"].as_str().unwrap_or_default().to_owned();
            let license = p["license"].as_str().unwrap_or("UNKNOWN").to_owned();
            if !license_ok(&license) {
                bad.push(format!("{name} {version}: {license}"));
            }
            rows.push((name, version, license));
        }
    }
    rows.sort();
    rows.dedup();
    if !bad.is_empty() {
        return Err(format!(
            "dependencies with licences outside the allow-list:\n  {}",
            bad.join("\n  ")
        ));
    }
    let mut md = String::from("# Third-party software\n\nGenerated by `cargo xtask licenses` from the lockfile. Every dependency must have a permissive licence (the allow-list is in `xtask/src/main.rs`); copyleft licences fail the gate.\n\n| Crate | Version | Licence |\n| --- | --- | --- |\n");
    for (n, v, l) in &rows {
        md.push_str(&format!("| {n} | {v} | {l} |\n"));
    }
    let path = root.join("docs/third-party.md");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if normalize_newlines(&current) != normalize_newlines(&md) {
        if check {
            return Err("docs/third-party.md is out of date; run `cargo xtask licenses`".into());
        }
        std::fs::write(&path, md).map_err(|e| e.to_string())?;
    }
    println!("licenses: ok ({} dependencies)", rows.len());
    Ok(())
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn normalize_newlines_converts_crlf_to_lf() {
        assert_eq!(super::normalize_newlines("a\r\nb\r\n"), "a\nb\n");
        assert_eq!(super::normalize_newlines("a\nb\n"), "a\nb\n");
    }
}
