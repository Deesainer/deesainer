use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    #[default]
    Editor,
    Commenter,
    Viewer,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Editor => "editor",
            Role::Commenter => "commenter",
            Role::Viewer => "viewer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub token: String,
    pub role: Role,
}

#[derive(Debug, Clone, Default)]
pub struct Access {
    editor: String,
    commenter: String,
    viewer: String,
    users: Vec<User>,
}

fn same_secret(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

impl Access {
    pub fn new(editor: &str) -> Self {
        Self {
            editor: editor.to_owned(),
            ..Self::default()
        }
    }

    pub fn set_secret(&mut self, role: Role, token: &str) {
        if self.editor.is_empty() || same_secret(token, &self.editor) {
            return;
        }
        match role {
            Role::Viewer => self.viewer = token.to_owned(),
            Role::Commenter => self.commenter = token.to_owned(),
            Role::Editor => {}
        }
    }

    pub fn set_users(&mut self, users: Vec<User>) {
        self.users = users;
    }

    pub fn users(&self) -> &[User] {
        &self.users
    }

    pub fn authenticate(&self, token: &str) -> Option<(Role, Option<String>)> {
        let mut found: Option<(Role, Option<String>)> = None;
        for u in &self.users {
            if !u.token.is_empty() && same_secret(token, &u.token) && found.is_none() {
                found = Some((u.role, Some(u.name.clone())));
            }
        }
        let shared = [
            (Role::Editor, &self.editor),
            (Role::Commenter, &self.commenter),
            (Role::Viewer, &self.viewer),
        ];
        for (role, secret) in shared {
            let ok = if role == Role::Editor {
                if secret.is_empty() {
                    self.users.is_empty()
                } else {
                    same_secret(token, secret)
                }
            } else {
                !secret.is_empty() && same_secret(token, secret)
            };
            if ok && found.is_none() {
                found = Some((role, None));
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_map_to_roles_and_users() {
        let mut a = Access::new("edit");
        a.set_secret(Role::Viewer, "view");
        a.set_secret(Role::Commenter, "comment");
        a.set_secret(Role::Viewer, "edit");
        a.set_users(vec![User {
            name: "Ada".into(),
            token: "ada-secret".into(),
            role: Role::Commenter,
        }]);
        assert_eq!(a.authenticate("edit"), Some((Role::Editor, None)));
        assert_eq!(a.authenticate("view"), Some((Role::Viewer, None)));
        assert_eq!(a.authenticate("comment"), Some((Role::Commenter, None)));
        assert_eq!(
            a.authenticate("ada-secret"),
            Some((Role::Commenter, Some("Ada".into())))
        );
        assert_eq!(a.authenticate("nope"), None);
        assert_eq!(
            Access::new("").authenticate("anything"),
            Some((Role::Editor, None))
        );
        let mut guarded = Access::new("");
        guarded.set_users(vec![User {
            name: "Ada".into(),
            token: "ada-secret".into(),
            role: Role::Editor,
        }]);
        assert_eq!(guarded.authenticate("anything"), None);
        assert!(guarded.authenticate("ada-secret").is_some());
        let mut open = Access::new("");
        open.set_secret(Role::Viewer, "view");
        assert_eq!(open.authenticate("view"), Some((Role::Editor, None)));
    }
}
