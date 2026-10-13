mod bench;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use base64::Engine as _;
use deesainer_automation::{control, files, McpServer};
use deesainer_engine::Session;
use serde_json::{json, Value};

type CliResult<T> = Result<T, Box<dyn std::error::Error>>;

const USAGE: &str = "usage: deesainer-cli <commands | exec | run | new | info | render | export | import-svg | import-fig | import-figma | serve | mcp | --version> (see the docs at the top of apps/deesainer-cli/src/main.rs)";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("deesainer-cli: {err}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(raw: &[String]) -> Self {
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            let a = &raw[i];
            if let Some(name) = a.strip_prefix("--") {
                let takes_value = !matches!(name, "save" | "save-token" | "standalone");
                let value = if takes_value {
                    raw.get(i + 1).cloned()
                } else {
                    None
                };
                if value.is_some() {
                    i += 1;
                }
                flags.push((name.to_owned(), value));
            } else {
                positional.push(a.clone());
            }
            i += 1;
        }
        Self { positional, flags }
    }

    fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.as_deref())
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| n == name)
    }

    fn pos(&self, i: usize) -> Result<&str, String> {
        self.positional
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| USAGE.to_owned())
    }
}

fn session_for(path: Option<&str>) -> CliResult<Session> {
    Ok(match path {
        Some(p) => files::open(Path::new(p))?,
        None => Session::new()?,
    })
}

fn decode(v: &Value) -> CliResult<Vec<u8>> {
    let data = v["data"].as_str().ok_or("reply has no data")?;
    Ok(base64::engine::general_purpose::STANDARD.decode(data)?)
}

fn run() -> CliResult<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = raw.split_first() else {
        eprintln!("{USAGE}");
        return Ok(());
    };
    let args = Args::parse(rest);
    match cmd.as_str() {
        "commands" => {
            let mut session = Session::new()?;
            let list = deesainer_automation::Backend::commands(&mut session)?;
            println!("{}", serde_json::to_string_pretty(&list)?);
        }
        "exec" => {
            let id = args.pos(0)?;
            let params: Value = match args.positional.get(1) {
                Some(raw) => serde_json::from_str(raw)?,
                None => Value::Null,
            };
            let file = args.flag("file");
            let mut session = session_for(file)?;
            let reply = session.dispatch(id, &params)?;
            if args.has("save") {
                let path = file.ok_or("--save needs --file")?;
                files::save_session(&session, Path::new(path))?;
            }
            println!("{}", serde_json::to_string_pretty(&reply)?);
        }
        "run" => {
            let file = args.pos(0)?;
            let script = args.pos(1)?;
            let mut session = if Path::new(file).exists() {
                files::open(Path::new(file))?
            } else {
                Session::new()?
            };
            let text = std::fs::read_to_string(script)?;
            let mut failed = 0;
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let reply = control::handle_line(&mut session, line);
                if reply.get("error").is_some() {
                    failed += 1;
                }
                println!("{reply}");
            }
            let out = args.flag("out").unwrap_or(file);
            files::save_session(&session, Path::new(out))?;
            if failed > 0 {
                return Err(format!(
                    "{failed} request(s) failed; the file was saved with the rest"
                )
                .into());
            }
        }
        "import-fig" => {
            let bytes = std::fs::read(args.pos(0)?)?;
            let (doc, report) = deesainer_io::import_fig(&bytes)?;
            files::save_document(&doc, Path::new(args.pos(1)?))?;
            println!(
                "{}",
                json!({
                    "pages": report.pages,
                    "layers": report.layers,
                    "styles": report.styles,
                    "variables": report.variables,
                    "overrides": report.overrides,
                    "skipped": report.skipped,
                })
            );
        }
        "import-figma" => {
            let src = args.pos(0)?;
            let out = args.pos(1)?;
            let token = args
                .flag("token")
                .map(str::to_owned)
                .or_else(deesainer_automation::figma_api::token)
                .ok_or(
                    "no Figma token: pass --token, set FIGMA_TOKEN, or save one with --save-token",
                )?;
            if args.has("save-token") {
                deesainer_automation::figma_api::save_token(&token)?;
            }
            let (doc, report, name) = deesainer_automation::figma_api::import(src, &token)?;
            files::save_document(&doc, Path::new(out))?;
            println!(
                "{}",
                json!({ "name": name, "pages": report.pages, "layers": report.layers, "skipped": report.skipped })
            );
        }
        "new" => {
            let out = args.pos(0)?;
            let mut session = Session::new()?;
            if let (Some(w), Some(h)) = (args.positional.get(1), args.positional.get(2)) {
                session.dispatch(
                    "file.new",
                    &json!({ "width": w.parse::<f64>()?, "height": h.parse::<f64>()? }),
                )?;
            }
            files::save_session(&session, Path::new(out))?;
        }
        "info" => {
            let mut session = files::open(Path::new(args.pos(0)?))?;
            let doc = session.snapshot();
            let pages: Vec<Value> = doc
                .pages()
                .iter()
                .filter_map(|p| doc.get(*p))
                .map(|p| {
                    json!({
                        "id": p.id,
                        "name": p.name,
                        "layers": doc.subtree_pre_order(p.id).map(|v| v.len().saturating_sub(1)).unwrap_or(0),
                    })
                })
                .collect();
            let components = session.dispatch("component.list", &Value::Null)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "pages": pages,
                    "nodes": doc.len(),
                    "styles": doc.styles().count(),
                    "variables": doc.variables().count(),
                    "assets": doc.assets().count(),
                    "components": components.as_array().map(Vec::len).unwrap_or(0),
                }))?
            );
        }
        "render" | "export" => {
            let mut session = files::open(Path::new(args.pos(0)?))?;
            let out = PathBuf::from(args.pos(1)?);
            let id = args.flag("id").map(str::parse::<u64>).transpose()?;
            let scale = args
                .flag("scale")
                .map(str::parse::<f64>)
                .transpose()?
                .unwrap_or(1.0);
            let ext = out
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png")
                .to_lowercase();
            let image_cmd = match ext.as_str() {
                "jpg" | "jpeg" => "export.jpeg",
                "webp" => "export.webp",
                _ => "export.png",
            };
            let mut params = json!({});
            if let Some(id) = id {
                params["id"] = json!(id);
            }
            let bytes = match ext.as_str() {
                "svg" => session.dispatch("export.svg", &params)?["svg"]
                    .as_str()
                    .unwrap_or("")
                    .as_bytes()
                    .to_vec(),
                "pdf" => {
                    let p = match id {
                        Some(id) => json!({ "ids": [id] }),
                        None => json!({}),
                    };
                    decode(&session.dispatch("export.pdf", &p)?)?
                }
                _ => {
                    params["scale"] = json!(scale);
                    decode(&session.dispatch(image_cmd, &params)?)?
                }
            };
            std::fs::write(&out, bytes)?;
        }
        "import-svg" => {
            let file = args.pos(0)?;
            let svg = std::fs::read_to_string(args.pos(1)?)?;
            let mut session = if Path::new(file).exists() {
                files::open(Path::new(file))?
            } else {
                Session::new()?
            };
            let name = Path::new(args.pos(1)?)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("SVG")
                .to_owned();
            let reply = session.dispatch("import.svg", &json!({ "data": svg, "name": name }))?;
            files::save_session(&session, Path::new(args.flag("out").unwrap_or(file)))?;
            println!("{reply}");
        }
        "plugin" => {
            let file = args.pos(0)?;
            let module = args.pos(1)?;
            let mut session = if Path::new(file).exists() {
                files::open(Path::new(file))?
            } else {
                Session::new()?
            };
            let bytes = std::fs::read(module)?;
            let entry = args.flag("entry").unwrap_or("run");
            let params = if module.ends_with(".wat") {
                json!({ "wat": String::from_utf8_lossy(&bytes), "entry": entry })
            } else {
                json!({ "data": base64::engine::general_purpose::STANDARD.encode(&bytes), "entry": entry })
            };
            let report = session.dispatch("plugin.run", &params)?;
            files::save_session(&session, Path::new(args.flag("out").unwrap_or(file)))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        "bench" => println!("{}", serde_json::to_string_pretty(&bench::run()?)?),
        "serve" => serve(session_for(args.flag("file"))?)?,
        "mcp" => {
            if let Some(addr) = args.flag("bridge") {
                let token_file = args
                    .flag("token-file")
                    .ok_or("--bridge needs --token-file FILE")?;
                let token = std::fs::read_to_string(token_file)?.trim().to_owned();
                let client =
                    deesainer_automation::control_server::ControlClient::connect(addr, &token)?;
                McpServer::new(client, "deesainer").run_stdio()?;
            } else if let Some(addr) = args.flag("join") {
                let name = args.flag("name").unwrap_or("Agent");
                let token = args.flag("token").unwrap_or("");
                let synced = deesainer_automation::SyncedSession::join(
                    addr,
                    name,
                    token,
                    std::time::Duration::from_secs(10),
                )?;
                McpServer::new(synced, "deesainer").run_stdio()?;
            } else if !args.has("standalone") {
                if let Some((port, token_file)) =
                    deesainer_automation::control_server::read_pointer()
                {
                    let addr = format!("127.0.0.1:{port}");
                    let token = std::fs::read_to_string(&token_file)
                        .map_err(|e| {
                            format!(
                                "the app published a control server but its token file {} is unreadable: {e}",
                                token_file.display()
                            )
                        })?
                        .trim()
                        .to_owned();
                    let client = deesainer_automation::control_server::ControlClient::connect(
                        &addr, &token,
                    )
                    .map_err(|e| {
                        deesainer_automation::control_server::clear_pointer();
                        format!(
                            "the app at {addr} did not answer ({e}); start Deesainer or run `deesainer-cli mcp --standalone`"
                        )
                    })?;
                    McpServer::new(client, "deesainer").run_stdio()?;
                } else {
                    let mut session = session_for(args.flag("file"))?;
                    session.set_clock(deesainer_automation::now_rfc3339);
                    session.set_user(args.flag("name").unwrap_or("Agent"));
                    McpServer::new(session, "deesainer").run_stdio()?;
                }
            } else {
                let mut session = session_for(args.flag("file"))?;
                session.set_clock(deesainer_automation::now_rfc3339);
                session.set_user(args.flag("name").unwrap_or("Agent"));
                McpServer::new(session, "deesainer").run_stdio()?;
            }
        }
        "--version" | "-V" => println!("deesainer-cli {}", env!("CARGO_PKG_VERSION")),
        _ => eprintln!("{USAGE}"),
    }
    Ok(())
}

fn serve(mut session: Session) -> CliResult<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = control::handle_line(&mut session, &line);
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}
