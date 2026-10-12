use std::io::Read;
use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value};

const MAX_BODY: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct TeamFile {
    pub id: u64,
    pub name: String,
    pub role: String,
}

pub struct TeamClient {
    pub base: String,
    pub token: String,
    pub name: String,
    agent: ureq::Agent,
}

fn check_base(base: &str) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if let Some(rest) = base.strip_prefix("http://") {
        let host = rest.split(':').next().unwrap_or("");
        if !(host == "127.0.0.1" || host == "localhost" || host == "[::1]") {
            return Err("use https:// for a server on another machine".into());
        }
    } else if !base.starts_with("https://") {
        return Err("the server address starts with https://".into());
    }
    Ok(base.to_owned())
}

fn agent() -> ureq::Agent {
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .http_status_as_error(false);
    if let Some(pem) = std::env::var_os(crate::tls::CA_ENV).and_then(|p| std::fs::read(p).ok()) {
        if let Ok(cert) = ureq::tls::Certificate::from_pem(&pem) {
            config = config.tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::new_with_certs(&[cert]))
                    .build(),
            );
        }
    }
    config.build().into()
}

fn answer(resp: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Value, String> {
    let resp = resp.map_err(|e| format!("cannot reach the server: {e}"))?;
    let status = resp.status().as_u16();
    let mut body = Vec::new();
    resp.into_body()
        .into_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if status >= 400 {
        return Err(value["error"]
            .as_str()
            .map_or_else(|| format!("the server answered {status}"), str::to_owned));
    }
    Ok(value)
}

impl TeamClient {
    pub fn login(base: &str, email: &str, password: &str) -> Result<Self, String> {
        let base = check_base(base)?;
        let agent = agent();
        let v = answer(
            agent
                .post(&format!("{base}/api/login"))
                .header("Content-Type", "application/json")
                .send(json!({ "email": email, "password": password }).to_string()),
        )?;
        Ok(Self {
            token: v["token"]
                .as_str()
                .ok_or("no session in the answer")?
                .to_owned(),
            name: v["user"]["name"].as_str().unwrap_or("").to_owned(),
            base,
            agent,
        })
    }

    pub fn resume(base: &str, token: &str) -> Result<Self, String> {
        let base = check_base(base)?;
        let agent = agent();
        let v = answer(
            agent
                .get(&format!("{base}/api/me"))
                .header("Authorization", &format!("Bearer {token}"))
                .call(),
        )?;
        Ok(Self {
            base,
            token: token.to_owned(),
            name: v["name"].as_str().unwrap_or("").to_owned(),
            agent,
        })
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        answer(
            self.agent
                .get(&format!("{}{path}", self.base))
                .header("Authorization", &format!("Bearer {}", self.token))
                .call(),
        )
    }

    fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        answer(
            self.agent
                .post(&format!("{}{path}", self.base))
                .header("Authorization", &format!("Bearer {}", self.token))
                .header("Content-Type", "application/json")
                .send(body.to_string()),
        )
    }

    pub fn files(&self) -> Result<Vec<TeamFile>, String> {
        let v = self.get("/api/files")?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| {
                        Some(TeamFile {
                            id: f["id"].as_u64()?,
                            name: f["name"].as_str()?.to_owned(),
                            role: f["role"].as_str().unwrap_or("").to_owned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn create(&self, name: &str, document: Option<&[u8]>) -> Result<u64, String> {
        let mut body = json!({ "name": name });
        if let Some(bytes) = document {
            body["data"] = json!(base64::engine::general_purpose::STANDARD.encode(bytes));
        }
        self.post("/api/files", body)?["id"]
            .as_u64()
            .ok_or_else(|| "no file id in the answer".into())
    }

    pub fn logout(&self) -> Result<(), String> {
        self.post("/api/logout", json!({})).map(|_| ())
    }

    pub fn room_address(&self) -> String {
        match self.base.strip_prefix("https://") {
            Some(host) => format!("tls://{host}"),
            None => self.base.trim_start_matches("http://").to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_need_tls_beyond_this_machine() {
        assert!(check_base("http://127.0.0.1:7800").is_ok());
        assert!(check_base("http://localhost:7800/").is_ok());
        assert!(check_base("http://team.example:7800").is_err());
        assert!(check_base("ftp://x").is_err());
        assert_eq!(
            check_base("https://team.example:7800/").unwrap(),
            "https://team.example:7800"
        );
    }
}
