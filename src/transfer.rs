//! Import / export: `.json`, `.env`, `.reg` and (export only) a PowerShell script.

use crate::model::{EnvSet, Scope, Var};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Json,
    DotEnv,
    Reg,
    PowerShell,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Json, Format::DotEnv, Format::Reg, Format::PowerShell];

    pub fn label(self) -> &'static str {
        match self {
            Format::Json => ".json",
            Format::DotEnv => ".env",
            Format::Reg => ".reg",
            Format::PowerShell => "PowerShell script",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::DotEnv => "env",
            Format::Reg => "reg",
            Format::PowerShell => "ps1",
        }
    }

    pub fn can_import(self) -> bool {
        self != Format::PowerShell
    }

    pub fn from_path(path: &str) -> Option<Format> {
        let lower = path.to_lowercase();
        Format::ALL.into_iter().find(|f| lower.ends_with(&format!(".{}", f.ext())))
    }
}

const REG_USER: &str = r"HKEY_CURRENT_USER\Environment";
const REG_SYSTEM: &str = r"HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

// ───────────────────────────── export ─────────────────────────────

pub fn export(env: &EnvSet, user: bool, system: bool, format: Format) -> Vec<u8> {
    let picked = EnvSet {
        user: if user { env.user.clone() } else { Vec::new() },
        system: if system { env.system.clone() } else { Vec::new() },
    };
    match format {
        Format::Json => serde_json::to_vec_pretty(&picked).unwrap_or_default(),
        Format::DotEnv => export_dotenv(&picked).into_bytes(),
        Format::Reg => {
            // regedit expects UTF-16LE with a BOM.
            let text = export_reg(&picked);
            let mut out = vec![0xFF, 0xFE];
            out.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
            out
        }
        Format::PowerShell => export_ps(&picked).into_bytes(),
    }
}

fn export_dotenv(env: &EnvSet) -> String {
    let mut s = String::from("# Exported by Envoy\n");
    for scope in Scope::ALL {
        let vars = env.get(scope);
        if vars.is_empty() {
            continue;
        }
        s.push_str(&format!("\n# --- {} ---\n", scope.lower()));
        for v in vars {
            let needs_quotes = v.value != v.value.trim()
                || v.value.contains(['#', '"', '\'', '\n'])
                || v.value.is_empty();
            if needs_quotes {
                let esc = v.value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                s.push_str(&format!("{}=\"{}\"\n", v.name, esc));
            } else {
                s.push_str(&format!("{}={}\n", v.name, v.value));
            }
        }
    }
    s
}

fn export_reg(env: &EnvSet) -> String {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut s = String::from("Windows Registry Editor Version 5.00\r\n");
    for scope in Scope::ALL {
        let vars = env.get(scope);
        if vars.is_empty() {
            continue;
        }
        let key = if scope == Scope::User { REG_USER } else { REG_SYSTEM };
        s.push_str(&format!("\r\n[{key}]\r\n"));
        for v in vars {
            if v.expand {
                let prefix = format!("\"{}\"=hex(2):", esc(&v.name));
                let bytes: Vec<String> = v
                    .value
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .flat_map(|u| u.to_le_bytes())
                    .map(|b| format!("{b:02x}"))
                    .collect();
                s.push_str(&prefix);
                // regedit wraps at ~80 columns with a trailing backslash.
                let mut col = prefix.len();
                for (i, b) in bytes.iter().enumerate() {
                    if col + 3 > 77 {
                        s.push_str("\\\r\n  ");
                        col = 2;
                    }
                    s.push_str(b);
                    if i + 1 < bytes.len() {
                        s.push(',');
                    }
                    col += 3;
                }
                s.push_str("\r\n");
            } else {
                s.push_str(&format!("\"{}\"=\"{}\"\r\n", esc(&v.name), esc(&v.value)));
            }
        }
    }
    s
}

fn export_ps(env: &EnvSet) -> String {
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let mut s = String::from(
        "# Exported by Envoy. Running this writes the variables below to the registry.\r\n\
         # System variables need an elevated PowerShell. Sign out and in (or restart apps) afterwards.\r\n",
    );
    for scope in Scope::ALL {
        let vars = env.get(scope);
        if vars.is_empty() {
            continue;
        }
        let key = match scope {
            Scope::User => r"HKCU:\Environment",
            Scope::System => r"HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        };
        s.push_str(&format!("\r\n# {} variables\r\n$key = {}\r\n", scope.label(), q(key)));
        for v in vars {
            let ty = if v.expand { "ExpandString" } else { "String" };
            s.push_str(&format!(
                "Set-ItemProperty -Path $key -Name {} -Value {} -Type {ty}\r\n",
                q(&v.name),
                q(&v.value)
            ));
        }
    }
    s
}

// ───────────────────────────── import ─────────────────────────────

pub fn import(bytes: &[u8], format: Format) -> Result<EnvSet, String> {
    match format {
        Format::Json => import_json(bytes),
        Format::DotEnv => Ok(import_dotenv(&decode_text(bytes))),
        Format::Reg => import_reg(&decode_text(bytes)),
        Format::PowerShell => Err("PowerShell scripts can be exported but not imported.".into()),
    }
}

fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        String::from_utf8_lossy(b).into_owned()
    }
}

fn import_json(bytes: &[u8]) -> Result<EnvSet, String> {
    // Accept an Envoy export, an Envoy backup, or a flat { "NAME": "value" } object.
    #[derive(serde::Deserialize)]
    struct Wrapped {
        env: EnvSet,
    }
    let text = decode_text(bytes);
    if let Ok(w) = serde_json::from_str::<Wrapped>(&text) {
        return Ok(w.env);
    }
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("Not valid JSON: {e}"))?;
    let obj = value.as_object().ok_or("Expected a JSON object.")?;
    if obj.contains_key("user") || obj.contains_key("system") {
        return serde_json::from_value(value).map_err(|e| format!("Unrecognised layout: {e}"));
    }
    let mut env = EnvSet::default();
    for (k, v) in obj {
        let s = v.as_str().ok_or_else(|| format!("\"{k}\" is not a string."))?;
        env.user.push(Var::new(k.clone(), s));
    }
    Ok(env)
}

fn import_dotenv(text: &str) -> EnvSet {
    let mut env = EnvSet::default();
    let mut scope = Scope::User;
    for line in text.lines() {
        let line = line.trim();
        if let Some(comment) = line.strip_prefix('#') {
            match comment.trim().to_lowercase().as_str() {
                "--- user ---" => scope = Scope::User,
                "--- system ---" => scope = Scope::System,
                _ => {}
            }
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, raw)) = line.split_once('=') else { continue };
        let (name, raw) = (name.trim(), raw.trim());
        if name.is_empty() {
            continue;
        }
        let value = if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
            unescape(&raw[1..raw.len() - 1], true)
        } else if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
            raw[1..raw.len() - 1].to_owned()
        } else {
            raw.to_owned()
        };
        env.set(scope, Var::new(name, value));
    }
    env
}

/// Undo `\\` and `\"` (and `\n` when `newlines`).
fn unescape(s: &str, newlines: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') if newlines => out.push('\n'),
            Some(n @ ('\\' | '"')) => out.push(n),
            Some(n) => {
                out.push('\\');
                out.push(n);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn import_reg(text: &str) -> Result<EnvSet, String> {
    if !text.trim_start().starts_with("Windows Registry Editor") && !text.trim_start().starts_with("REGEDIT4") {
        return Err("This does not look like a .reg file.".into());
    }
    // Join lines continued with a trailing backslash.
    let mut lines: Vec<String> = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(head) = t.strip_suffix('\\').filter(|_| !t.starts_with('[') && pending_or_hex(&pending, t)) {
            pending.push_str(head);
        } else {
            pending.push_str(t);
            lines.push(std::mem::take(&mut pending));
        }
    }

    let mut env = EnvSet::default();
    let mut scope: Option<Scope> = None;
    for line in &lines {
        if line.starts_with('[') {
            let key = line.trim_matches(['[', ']']);
            scope = if key.eq_ignore_ascii_case(REG_USER) {
                Some(Scope::User)
            } else if key.eq_ignore_ascii_case(REG_SYSTEM) {
                Some(Scope::System)
            } else {
                None
            };
            continue;
        }
        let Some(scope) = scope else { continue };
        let Some((name, rest)) = parse_quoted(line) else { continue };
        let Some(rest) = rest.trim_start().strip_prefix('=') else { continue };
        let rest = rest.trim();
        let parsed = if rest.starts_with('"') {
            parse_quoted(rest).map(|(v, _)| (v, false))
        } else if let Some(hex) = rest.strip_prefix("hex(2):") {
            parse_hex_utf16(hex).map(|v| (v, true))
        } else if let Some(hex) = rest.strip_prefix("hex(1):") {
            parse_hex_utf16(hex).map(|v| (v, false))
        } else {
            None // "-" (delete), dword, binary: not environment strings
        };
        if let Some((value, expand)) = parsed
            && !name.is_empty()
        {
            env.set(scope, Var { name, value, expand });
        }
    }
    Ok(env)
}

fn pending_or_hex(pending: &str, line: &str) -> bool {
    !pending.is_empty() || line.contains("=hex")
}

/// Parse a leading `"…"` with reg escapes; returns the content and the remainder.
fn parse_quoted(s: &str) -> Option<(String, &str)> {
    let body = s.strip_prefix('"')?;
    let mut escaped = false;
    for (i, c) in body.char_indices() {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return Some((unescape(&body[..i], false), &body[i + 1..])),
            _ => escaped = false,
        }
    }
    None
}

fn parse_hex_utf16(hex: &str) -> Option<String> {
    let bytes: Option<Vec<u8>> = hex
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| u8::from_str_radix(p, 16).ok())
        .collect();
    let bytes = bytes?;
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    Some(String::from_utf16_lossy(&units[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> EnvSet {
        EnvSet {
            user: vec![
                Var::new("Path", r"%USERPROFILE%\bin;C:\Tools\a very long folder name\bin;C:\x\y\z;D:\more\stuff\here"),
                Var::new("QUOTE", r#"say "hi" \ # not a comment"#),
                Var::new("EDITOR", "code --wait"),
            ],
            system: vec![Var::new("JAVA_HOME", r"C:\Program Files\Java\jdk-17"), Var::new("TEMP", r"%SystemRoot%\TEMP")],
        }
    }

    #[test]
    fn round_trips() {
        let env = sample();
        for f in [Format::Json, Format::DotEnv, Format::Reg] {
            let bytes = export(&env, true, true, f);
            let back = import(&bytes, f).unwrap_or_else(|e| panic!("{f:?}: {e}"));
            assert_eq!(back, env, "{f:?}");
        }
    }

    #[test]
    fn scope_filter_and_flat_json() {
        let only_sys = import(&export(&sample(), false, true, Format::Reg), Format::Reg).unwrap();
        assert!(only_sys.user.is_empty() && only_sys.system.len() == 2);
        let flat = import(br#"{"A":"1","B":"%A%"}"#, Format::Json).unwrap();
        assert_eq!(flat.user.len(), 2);
        assert!(flat.user[1].expand);
        assert!(import(b"x", Format::PowerShell).is_err());
    }

    #[test]
    fn powershell_quotes() {
        let env = EnvSet { user: vec![Var::new("A", "it's")], system: vec![] };
        let ps = String::from_utf8(export(&env, true, true, Format::PowerShell)).unwrap();
        assert!(ps.contains("-Name 'A' -Value 'it''s' -Type String"));
    }
}
