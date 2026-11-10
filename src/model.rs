//! Core data types: variables, scopes, diffing and `%VAR%` expansion.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Windows caps a single environment variable at 32,767 UTF-16 units.
pub const MAX_VALUE_LEN: usize = 32_767;
/// The length the built-in Windows editor stops at.
pub const NATIVE_EDITOR_LIMIT: usize = 2_047;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    User,
    System,
}

impl Scope {
    pub const ALL: [Scope; 2] = [Scope::User, Scope::System];

    pub fn label(self) -> &'static str {
        match self {
            Scope::User => "User",
            Scope::System => "System",
        }
    }

    pub fn lower(self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::System => "system",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Var {
    pub name: String,
    pub value: String,
    /// Stored as `REG_EXPAND_SZ` (Windows expands `%VAR%` references) rather than `REG_SZ`.
    #[serde(default)]
    pub expand: bool,
}

impl Var {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        let value = value.into();
        Var {
            name: name.into(),
            expand: has_reference(&value),
            value,
        }
    }

    /// Length as Windows counts it.
    pub fn len_utf16(&self) -> usize {
        self.value.encode_utf16().count()
    }
}

#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
pub struct EnvSet {
    #[serde(default)]
    pub user: Vec<Var>,
    #[serde(default)]
    pub system: Vec<Var>,
}

impl EnvSet {
    pub fn get(&self, scope: Scope) -> &Vec<Var> {
        match scope {
            Scope::User => &self.user,
            Scope::System => &self.system,
        }
    }

    pub fn get_mut(&mut self, scope: Scope) -> &mut Vec<Var> {
        match scope {
            Scope::User => &mut self.user,
            Scope::System => &mut self.system,
        }
    }

    pub fn find(&self, scope: Scope, name: &str) -> Option<&Var> {
        self.get(scope).iter().find(|v| ieq(&v.name, name))
    }

    pub fn find_mut(&mut self, scope: Scope, name: &str) -> Option<&mut Var> {
        self.get_mut(scope).iter_mut().find(|v| ieq(&v.name, name))
    }

    /// Insert or replace (matching the name case-insensitively).
    pub fn set(&mut self, scope: Scope, var: Var) {
        match self.find_mut(scope, &var.name) {
            Some(v) => *v = var,
            None => self.get_mut(scope).push(var),
        }
    }

    pub fn remove(&mut self, scope: Scope, name: &str) {
        self.get_mut(scope).retain(|v| !ieq(&v.name, name));
    }

    pub fn total(&self) -> usize {
        self.user.len() + self.system.len()
    }
}

/// Environment variable names are case-insensitive on Windows.
pub fn ieq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase()
}

/// True when `s` contains something shaped like a `%NAME%` reference.
pub fn has_reference(s: &str) -> bool {
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => {
                let name = &after[..j];
                if !name.is_empty() && !name.contains([' ', ';', '\\', '/', '=']) {
                    return true;
                }
                rest = &after[j..];
            }
            None => return false,
        }
    }
    false
}

// ───────────────────────────── lists ─────────────────────────────

const ALWAYS_LIST: [&str; 3] = ["PATH", "PATHEXT", "PSMODULEPATH"];

pub fn looks_like_list(name: &str, value: &str) -> bool {
    value.contains(';') || ALWAYS_LIST.iter().any(|n| name.eq_ignore_ascii_case(n))
}

pub fn split_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn join_list(items: &[String]) -> String {
    items.join(";")
}

pub fn looks_secret(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    const NEEDLES: [&str; 12] = [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "PASSPHRASE",
        "APIKEY",
        "API_KEY",
        "ACCESS_KEY",
        "PRIVATE_KEY",
        "CREDENTIAL",
        "CLIENT_KEY",
        "AUTH_KEY",
    ];
    NEEDLES.iter().any(|k| n.contains(k))
        || ["_PWD", "_PAT", "_PASS", "_KEY"].iter().any(|s| n.ends_with(s))
        || n.contains("_PASS_")

}

/// `C:\…` or `C:/…` — the only shape the background validator checks on disk.
pub fn is_abs_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// Key used to compare two paths for equality (duplicates, validation cache).
pub fn path_key(s: &str) -> String {
    let t = s.trim().trim_matches('"').replace('/', "\\");
    let t = if t.len() > 3 { t.trim_end_matches('\\') } else { &t };
    t.to_lowercase()
}

// ───────────────────────────── diff ─────────────────────────────

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum ChangeOp {
    Set { value: String, expand: bool },
    Delete,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Change {
    pub scope: Scope,
    pub name: String,
    #[serde(flatten)]
    pub op: ChangeOp,
}

/// The registry writes needed to turn `saved` into `work`. Only variables that
/// actually differ are touched, so anything changed outside Envoy is left alone.
pub fn diff(saved: &EnvSet, work: &EnvSet) -> Vec<Change> {
    let mut out = Vec::new();
    for scope in Scope::ALL {
        let (old, new) = (saved.get(scope), work.get(scope));
        for o in old {
            if !new.iter().any(|n| ieq(&n.name, &o.name)) {
                out.push(Change { scope, name: o.name.clone(), op: ChangeOp::Delete });
            }
        }
        for n in new {
            let set = || Change {
                scope,
                name: n.name.clone(),
                op: ChangeOp::Set { value: n.value.clone(), expand: n.expand },
            };
            match old.iter().find(|o| ieq(&o.name, &n.name)) {
                None => out.push(set()),
                Some(o) if o.name != n.name => {
                    // Only the letter case changed: the registry keeps the old
                    // spelling unless the value is removed first.
                    out.push(Change { scope, name: o.name.clone(), op: ChangeOp::Delete });
                    out.push(set());
                }
                Some(o) if o.value != n.value || o.expand != n.expand => out.push(set()),
                Some(_) => {}
            }
        }
    }
    out
}

/// Replay `pending` on top of a freshly read environment, so edits survive a
/// reload without undoing whatever changed in the registry meanwhile.
pub fn rebase(fresh: &EnvSet, pending: &[Change]) -> EnvSet {
    let mut env = fresh.clone();
    for c in pending {
        match &c.op {
            ChangeOp::Delete => env.remove(c.scope, &c.name),
            ChangeOp::Set { value, expand } => {
                env.set(c.scope, Var { name: c.name.clone(), value: value.clone(), expand: *expand })
            }
        }
    }
    env
}

/// How many variables the change list touches (a case-only rename counts once).
pub fn changed_var_count(changes: &[Change]) -> usize {
    let mut seen: Vec<(Scope, String)> = Vec::new();
    for c in changes {
        let key = (c.scope, c.name.to_lowercase());
        if !seen.contains(&key) {
            seen.push(key);
        }
    }
    seen.len()
}

// ───────────────────────────── expansion ─────────────────────────────

/// Resolves `%VAR%` references the way a freshly started process would see them.
///
/// System variables can only see other system variables; user variables see
/// system variables overlaid with user ones.
pub struct Expander {
    sys: HashMap<String, String>,
    user: HashMap<String, String>,
}

impl Expander {
    pub fn new(env: &EnvSet) -> Self {
        // Volatile variables (USERPROFILE, APPDATA, ProgramFiles…) are not stored
        // under the Environment keys, so they come from this process.
        let mut sys: HashMap<String, String> = std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?.to_lowercase(), v.into_string().ok()?)))
            .collect();
        for v in &env.user {
            sys.remove(&v.name.to_lowercase());
        }
        for v in &env.system {
            sys.insert(v.name.to_lowercase(), v.value.clone());
        }
        let mut user = sys.clone();
        for v in &env.user {
            user.insert(v.name.to_lowercase(), v.value.clone());
        }
        // A reference to the variable itself is never useful to resolve.
        Expander { sys, user }
    }

    pub fn expand(&self, scope: Scope, s: &str) -> String {
        let map = match scope {
            Scope::User => &self.user,
            Scope::System => &self.sys,
        };
        let mut cur = s.to_owned();
        for _ in 0..6 {
            if !cur.contains('%') {
                break;
            }
            let next = expand_once(map, &cur);
            if next == cur {
                break;
            }
            cur = next;
        }
        cur
    }
}

fn expand_once(map: &HashMap<String, String>, s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => match map.get(&after[..j].to_lowercase()) {
                Some(v) if j > 0 => {
                    out.push_str(v);
                    rest = &after[j + 1..];
                }
                _ => {
                    out.push('%');
                    rest = after;
                }
            },
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> EnvSet {
        EnvSet {
            user: vec![
                Var::new("GOPATH", r"%ENVOY_T_ROOT%\go"),
                Var::new("Path", r"%GOPATH%\bin;C:\x"),
            ],
            system: vec![Var::new("ENVOY_T_ROOT", r"C:\Root"), Var::new("Path", r"%GOPATH%\sys")],
        }
    }

    #[test]
    fn expands_nested_and_respects_scope() {
        let e = Expander::new(&env());
        assert_eq!(e.expand(Scope::User, r"%gopath%\bin"), r"C:\Root\go\bin");
        // System scope cannot see the user-only GOPATH.
        assert_eq!(e.expand(Scope::System, r"%GOPATH%\sys"), r"%GOPATH%\sys");
        assert_eq!(e.expand(Scope::User, "100% %nope% %"), "100% %nope% %");
    }

    #[test]
    fn diff_only_touches_what_changed() {
        let saved = env();
        let mut work = saved.clone();
        assert!(diff(&saved, &work).is_empty());
        work.remove(Scope::User, "gopath");
        work.set(Scope::System, Var::new("NEW", "1"));
        work.find_mut(Scope::User, "PATH").unwrap().name = "PATH".into();
        let d = diff(&saved, &work);
        assert_eq!(d.len(), 4);
        assert_eq!(changed_var_count(&d), 3);
        assert!(d.contains(&Change { scope: Scope::User, name: "GOPATH".into(), op: ChangeOp::Delete }));
        assert!(d.contains(&Change { scope: Scope::User, name: "Path".into(), op: ChangeOp::Delete }));
    }

    #[test]
    fn helpers() {
        assert!(has_reference(r"%A%\bin"));
        assert!(!has_reference("50% off; 20% more"));
        assert_eq!(split_list("a; b;;c;"), vec!["a", "b", "c"]);
        assert!(looks_like_list("Path", "single"));
        assert!(looks_secret("GITHUB_TOKEN") && !looks_secret("EDITOR"));
        assert_eq!(path_key(r#""C:/Tools/Bin\""#), r"c:\tools\bin");
        assert!(is_abs_path(r"C:\x") && !is_abs_path(".COM"));
    }
}
