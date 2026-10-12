use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SESSION_SECS: u64 = 30 * 24 * 3600;
pub const INVITE_SECS: u64 = 7 * 24 * 3600;
const MAX_FAILURES: u32 = 5;
const LOCKOUT_SECS: u64 = 300;

pub type UserId = u64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: UserId,
    pub email: String,
    pub name: String,
    password: String,
    pub admin: bool,
    pub created: u64,
    #[serde(default)]
    failures: u32,
    #[serde(default)]
    locked_until: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Invite {
    code: String,
    email: Option<String>,
    admin: bool,
    expires: u64,
    by: UserId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionRec {
    token: String,
    user: UserId,
    expires: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    users: Vec<User>,
    invites: Vec<Invite>,
    sessions: Vec<SessionRec>,
    next_id: UserId,
}

pub struct Accounts {
    path: PathBuf,
    store: Store,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountError {
    BadLogin,
    Locked,
    BadInvite,
    Taken,
    Weak,
    Invalid(String),
    Io(String),
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountError::BadLogin => write!(f, "wrong email or password"),
            AccountError::Locked => write!(f, "too many failed logins, try again in a few minutes"),
            AccountError::BadInvite => write!(f, "this invite is not valid or has expired"),
            AccountError::Taken => write!(f, "an account with this email exists"),
            AccountError::Weak => write!(f, "the password needs at least 10 characters"),
            AccountError::Invalid(why) => write!(f, "{why}"),
            AccountError::Io(e) => write!(f, "cannot store the accounts: {e}"),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn random_token() -> Result<String, AccountError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| AccountError::Io(e.to_string()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn digest(secret: &str) -> String {
    let h = Sha256::digest(secret.as_bytes());
    h.iter().map(|b| format!("{b:02x}")).collect()
}

fn normalize_email(email: &str) -> Result<String, AccountError> {
    let e = email.trim().to_lowercase();
    let ok = e.len() <= 254
        && e.split_once('@')
            .is_some_and(|(a, b)| !a.is_empty() && b.contains('.') && !b.starts_with('.'));
    if ok {
        Ok(e)
    } else {
        Err(AccountError::Invalid("that is not an email address".into()))
    }
}

fn hash_password(password: &str) -> Result<String, AccountError> {
    if password.chars().count() < 10 {
        return Err(AccountError::Weak);
    }
    if password.len() > 1024 {
        return Err(AccountError::Invalid("the password is too long".into()));
    }
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| AccountError::Io(e.to_string()))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| AccountError::Io(e.to_string()))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AccountError::Io(e.to_string()))
}

fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| hash_password("no account has this password").unwrap_or_default())
}

fn password_matches(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}

impl Accounts {
    pub fn open(path: &Path) -> Result<Self, AccountError> {
        let store = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| AccountError::Io(format!("{}: {e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::default(),
            Err(e) => return Err(AccountError::Io(e.to_string())),
        };
        Ok(Self {
            path: path.to_owned(),
            store,
        })
    }

    fn save(&mut self) -> Result<(), AccountError> {
        let t = now();
        self.store.sessions.retain(|s| s.expires > t);
        self.store.invites.retain(|i| i.expires > t);
        let bytes =
            serde_json::to_vec_pretty(&self.store).map_err(|e| AccountError::Io(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| AccountError::Io(e.to_string()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| AccountError::Io(e.to_string()))
    }

    pub fn users(&self) -> &[User] {
        &self.store.users
    }

    pub fn user(&self, id: UserId) -> Option<&User> {
        self.store.users.iter().find(|u| u.id == id)
    }

    fn add_user(
        &mut self,
        email: &str,
        name: &str,
        password: &str,
        admin: bool,
    ) -> Result<UserId, AccountError> {
        let email = normalize_email(email)?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(AccountError::Invalid(
                "the name needs 1 to 64 characters".into(),
            ));
        }
        if self.store.users.iter().any(|u| u.email == email) {
            return Err(AccountError::Taken);
        }
        let password = hash_password(password)?;
        self.store.next_id = self.store.next_id.max(1);
        let id = self.store.next_id;
        self.store.next_id += 1;
        self.store.users.push(User {
            id,
            email,
            name: name.to_owned(),
            password,
            admin,
            created: now(),
            failures: 0,
            locked_until: 0,
        });
        Ok(id)
    }

    pub fn bootstrap_admin(
        &mut self,
        email: &str,
        name: &str,
        password: &str,
    ) -> Result<UserId, AccountError> {
        let id = self.add_user(email, name, password, true)?;
        self.save()?;
        Ok(id)
    }

    pub fn invite(
        &mut self,
        by: UserId,
        email: Option<&str>,
        admin: bool,
        secs: u64,
    ) -> Result<String, AccountError> {
        if !self.user(by).is_some_and(|u| u.admin) {
            return Err(AccountError::Invalid("only administrators invite".into()));
        }
        let email = email.map(normalize_email).transpose()?;
        let code = random_token()?;
        self.store.invites.push(Invite {
            code: digest(&code),
            email,
            admin,
            expires: now().saturating_add(secs.clamp(60, 90 * 24 * 3600)),
            by,
        });
        self.save()?;
        Ok(code)
    }

    pub fn accept(
        &mut self,
        code: &str,
        email: &str,
        name: &str,
        password: &str,
    ) -> Result<UserId, AccountError> {
        let hashed = digest(code);
        let t = now();
        let at = self
            .store
            .invites
            .iter()
            .position(|i| i.code == hashed && i.expires > t)
            .ok_or(AccountError::BadInvite)?;
        let wanted = normalize_email(email)?;
        if self.store.invites[at]
            .email
            .as_ref()
            .is_some_and(|e| *e != wanted)
        {
            return Err(AccountError::BadInvite);
        }
        let admin = self.store.invites[at].admin;
        let id = self.add_user(&wanted, name, password, admin)?;
        self.store.invites.remove(at);
        self.save()?;
        Ok(id)
    }

    pub fn login(&mut self, email: &str, password: &str) -> Result<String, AccountError> {
        let email = normalize_email(email).map_err(|_| AccountError::BadLogin)?;
        let t = now();
        let Some(user) = self.store.users.iter_mut().find(|u| u.email == email) else {
            let _ = password_matches(dummy_hash(), password);
            return Err(AccountError::BadLogin);
        };
        if user.locked_until > t {
            return Err(AccountError::Locked);
        }
        if !password_matches(&user.password, password) {
            user.failures += 1;
            if user.failures >= MAX_FAILURES {
                user.failures = 0;
                user.locked_until = t + LOCKOUT_SECS;
            }
            self.save()?;
            return Err(AccountError::BadLogin);
        }
        user.failures = 0;
        let id = user.id;
        let token = random_token()?;
        self.store.sessions.push(SessionRec {
            token: digest(&token),
            user: id,
            expires: t + SESSION_SECS,
        });
        self.save()?;
        Ok(token)
    }

    pub fn session(&self, token: &str) -> Option<&User> {
        if token.is_empty() {
            return None;
        }
        let hashed = digest(token);
        let t = now();
        let s = self
            .store
            .sessions
            .iter()
            .find(|s| s.token == hashed && s.expires > t)?;
        self.user(s.user)
    }

    pub fn logout(&mut self, token: &str) -> Result<(), AccountError> {
        let hashed = digest(token);
        self.store.sessions.retain(|s| s.token != hashed);
        self.save()
    }

    pub fn remove_user(&mut self, id: UserId) -> Result<(), AccountError> {
        self.store.users.retain(|u| u.id != id);
        self.store.sessions.retain(|s| s.user != id);
        self.save()
    }

    pub fn sessions_by_user(&self) -> BTreeMap<UserId, usize> {
        let mut out = BTreeMap::new();
        for s in &self.store.sessions {
            *out.entry(s.user).or_default() += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("deesainer-accounts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("accounts.json")
    }

    #[test]
    fn invites_logins_sessions_and_lockout() {
        let path = temp("cycle");
        let mut a = Accounts::open(&path).unwrap();
        let admin = a
            .bootstrap_admin("Ada@Example.com", "Ada", "correct horse battery")
            .unwrap();
        assert!(a
            .bootstrap_admin("ada@example.com", "Ada 2", "another long one")
            .is_err());
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("correct horse"));
        let code = a
            .invite(admin, Some("bob@example.com"), false, INVITE_SECS)
            .unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains(&code));
        assert_eq!(
            a.accept(&code, "eve@example.com", "Eve", "a long password"),
            Err(AccountError::BadInvite)
        );
        assert_eq!(
            a.accept(&code, "bob@example.com", "Bob", "short"),
            Err(AccountError::Weak)
        );
        let bob = a
            .accept(&code, "bob@example.com", "Bob", "a long password")
            .unwrap();
        assert_eq!(
            a.accept(&code, "bob@example.com", "Bob", "a long password"),
            Err(AccountError::BadInvite)
        );
        assert!(!a.user(bob).unwrap().admin);
        assert!(a.invite(bob, None, false, INVITE_SECS).is_err());
        let token = a.login("BOB@example.com", "a long password").unwrap();
        assert_eq!(a.session(&token).map(|u| u.id), Some(bob));
        let mut reopened = Accounts::open(&path).unwrap();
        assert_eq!(reopened.session(&token).map(|u| u.id), Some(bob));
        reopened.logout(&token).unwrap();
        assert!(reopened.session(&token).is_none());
        assert_eq!(
            reopened.login("nobody@example.com", "x"),
            Err(AccountError::BadLogin)
        );
        for _ in 0..5 {
            assert_eq!(
                reopened.login("bob@example.com", "wrong password"),
                Err(AccountError::BadLogin)
            );
        }
        assert_eq!(
            reopened.login("bob@example.com", "a long password"),
            Err(AccountError::Locked)
        );
        let t2 = reopened
            .login("ada@example.com", "correct horse battery")
            .unwrap();
        reopened.remove_user(admin).unwrap();
        assert!(reopened.session(&t2).is_none());
    }
}
