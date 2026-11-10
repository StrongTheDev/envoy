//! Application state and everything that changes it. Drawing lives in `views` and `dialogs`.

use crate::dialogs::Dialog;
use crate::model::*;
use crate::store::{self, Settings};
use crate::theme::{self, Palette};
use crate::validate::Validator;
use crate::winenv::{self, ElevateError};
use chrono::{DateTime, Local};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    User,
    System,
    Conflicts,
}

impl Tab {
    pub fn scope(self) -> Scope {
        if self == Tab::System { Scope::System } else { Scope::User }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Lists,
    Secrets,
    Issues,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Issue {
    NotFound,
    Unresolved,
    Duplicate,
}

impl Issue {
    pub fn label(self) -> &'static str {
        match self {
            Issue::NotFound => "Folder not found",
            Issue::Unresolved => "Refers to a variable that is not defined",
            Issue::Duplicate => "Duplicate of an entry above",
        }
    }
}

pub struct EntryInfo {
    pub raw: String,
    /// Equal to `raw` when nothing was expanded.
    pub exp: String,
    pub issue: Option<Issue>,
}

pub struct VarInfo {
    pub var: Var,
    pub is_list: bool,
    pub secret: bool,
    pub exp: String,
    /// Only for lists.
    pub entries: Vec<EntryInfo>,
    /// Tooltip text when something is wrong with this variable.
    pub issue: Option<String>,
    pub len: usize,
}

pub struct Conflict {
    pub name: String,
    /// PATH is joined (system first); everything else is replaced by the user value.
    pub merged: bool,
    pub secret: bool,
    pub sys: String,
    pub usr: String,
    pub eff: String,
}

/// Everything the UI shows that is computed from the working set. Rebuilt only when something changes.
#[derive(Default)]
pub struct Derived {
    pub user: Vec<VarInfo>,
    pub system: Vec<VarInfo>,
    pub conflicts: Vec<Conflict>,
    pub changes: Vec<Change>,
    pub paths: Vec<String>,
    pub bad_paths: usize,
}

impl Derived {
    pub fn vars(&self, scope: Scope) -> &Vec<VarInfo> {
        match scope {
            Scope::User => &self.user,
            Scope::System => &self.system,
        }
    }

    pub fn find(&self, scope: Scope, name: &str) -> Option<&VarInfo> {
        self.vars(scope).iter().find(|v| ieq(&v.var.name, name))
    }
}

pub struct Toast {
    pub title: String,
    pub body: String,
    pub error: bool,
    pub until: f64,
}

#[derive(Default)]
pub struct StartArgs {
    /// Pending edits handed over by the non-elevated instance.
    pub session: Option<PathBuf>,
    /// Who started the non-elevated instance.
    pub from_user: Option<String>,
}

struct ApplyOutcome {
    applied_any: bool,
    /// Changes that did not make it (declined UAC prompt, or an error).
    failed: Vec<Change>,
    cancelled: bool,
    errors: Vec<String>,
    fresh: Option<EnvSet>,
}

pub struct App {
    pub pal: Palette,
    pub settings: Settings,
    /// What the registry held when last read.
    pub saved: EnvSet,
    /// `saved` plus the user's pending edits.
    pub work: EnvSet,
    pub derived: Rc<Derived>,
    stale: bool,
    pub validator: Validator,

    pub tab: Tab,
    pub query: String,
    pub filter: Filter,
    /// The list variable open in the side pane, per scope.
    pub sel: [Option<String>; 2],
    /// Checked entries of the open list, by position (a list may hold duplicates).
    pub checked: HashSet<usize>,
    pub reveal: HashSet<(Scope, String)>,
    pub new_entry: String,
    /// Index of the list entry being dragged.
    pub drag: Option<usize>,
    pub dialog: Dialog,
    pub toast: Option<Toast>,

    pub elevated: bool,
    pub user: String,
    /// Set when elevation switched to another account: that account's user variables are shown.
    pub foreign_user: Option<String>,
    pub os: String,
    pub last_backup: Option<DateTime<Local>>,
    pub load_error: Option<String>,
    apply_rx: Option<Receiver<ApplyOutcome>>,
    was_focused: bool,
}

fn scope_idx(scope: Scope) -> usize {
    scope as usize
}

impl App {
    pub fn new(ctx: &egui::Context, args: StartArgs) -> Self {
        let settings = Settings::load();
        let dark = settings.dark.unwrap_or_else(|| ctx.system_theme() == Some(egui::Theme::Dark));
        let pal = Palette::of(dark);
        theme::apply(ctx, &pal);

        let (saved, load_error) = match winenv::read_all() {
            Ok(env) => (env, None),
            Err(e) => (EnvSet::default(), Some(e.to_string())),
        };
        let elevated = winenv::is_elevated();
        let user = winenv::user_name();
        let foreign_user = args.from_user.filter(|from| elevated && !ieq(from, &user));

        let mut work = saved.clone();
        if let Some(file) = &args.session {
            if let Some(session) = std::fs::read(file).ok().and_then(|b| serde_json::from_slice::<EnvSet>(&b).ok()) {
                work.system = session.system;
                if foreign_user.is_none() {
                    work.user = session.user;
                }
            }
            let _ = std::fs::remove_file(file);
        }

        App {
            pal,
            settings,
            saved,
            work,
            derived: Rc::new(Derived::default()),
            stale: true,
            validator: Validator::default(),
            tab: Tab::User,
            query: String::new(),
            filter: Filter::All,
            sel: [Some("Path".into()), Some("Path".into())],
            checked: HashSet::new(),
            reveal: HashSet::new(),
            new_entry: String::new(),
            drag: None,
            dialog: Dialog::None,
            toast: None,
            elevated,
            user,
            foreign_user,
            os: winenv::os_label(),
            last_backup: store::list_backups().first().map(|b| b.backup.created),
            load_error,
            apply_rx: None,
            was_focused: true,
        }
    }

    // ───────────────────────────── per-frame upkeep ─────────────────────────────

    pub fn tick(&mut self, ctx: &egui::Context) {
        if self.validator.poll() {
            self.stale = true;
        }
        self.poll_apply(ctx);

        let focused = ctx.input(|i| i.focused);
        if focused && !self.was_focused && self.apply_rx.is_none() {
            self.sync_external();
        }
        self.was_focused = focused;

        if let Some(t) = &self.toast {
            let now = ctx.input(|i| i.time);
            if now >= t.until {
                self.toast = None;
            } else {
                ctx.request_repaint_after_secs((t.until - now) as f32 + 0.05);
            }
        }

        if self.stale {
            self.stale = false;
            let derived = self.compute();
            self.validator.request(&derived.paths, ctx);
            self.derived = Rc::new(derived);
        }
    }

    /// Call after any change to `work`, the settings or the validator.
    pub fn touch(&mut self) {
        self.stale = true;
    }

    pub fn is_list(&self, name: &str, value: &str) -> bool {
        self.settings
            .list
            .get(&name.to_lowercase())
            .copied()
            .unwrap_or_else(|| looks_like_list(name, value))
    }

    pub fn is_secret(&self, name: &str) -> bool {
        self.settings.secret.get(&name.to_lowercase()).copied().unwrap_or_else(|| looks_secret(name))
    }

    fn compute(&self) -> Derived {
        let ex = Expander::new(&self.work);
        let mut d = Derived::default();
        for scope in Scope::ALL {
            let mut infos = Vec::new();
            for var in self.work.get(scope) {
                let is_list = self.is_list(&var.name, &var.value);
                let exp = if var.expand { ex.expand(scope, &var.value) } else { var.value.clone() };
                let len = var.len_utf16();
                let mut notes: Vec<String> = Vec::new();
                let mut entries = Vec::new();
                if is_list {
                    let mut seen = HashSet::new();
                    let (mut missing, mut unresolved, mut dupes) = (0, 0, 0);
                    for raw in split_list(&var.value) {
                        let e = if var.expand { ex.expand(scope, &raw) } else { raw.clone() };
                        let issue = if !seen.insert(path_key(&e)) {
                            dupes += 1;
                            Some(Issue::Duplicate)
                        } else if var.expand && has_reference(&e) {
                            unresolved += 1;
                            Some(Issue::Unresolved)
                        } else if is_abs_path(&e) {
                            d.paths.push(e.clone());
                            (self.validator.exists(&e) == Some(false)).then(|| {
                                missing += 1;
                                Issue::NotFound
                            })
                        } else {
                            None
                        };
                        entries.push(EntryInfo { raw, exp: e, issue });
                    }
                    let plural = |n: usize, one: &str, many: &str| {
                        format!("{n} {}", if n == 1 { one } else { many })
                    };
                    if missing > 0 {
                        notes.push(plural(missing, "folder not found", "folders not found"));
                    }
                    if unresolved > 0 {
                        notes.push(plural(unresolved, "entry refers", "entries refer") + " to an undefined variable");
                    }
                    if dupes > 0 {
                        notes.push(plural(dupes, "duplicate entry", "duplicate entries"));
                    }
                    d.bad_paths += missing;
                } else if var.expand && has_reference(&exp) {
                    notes.push("Refers to a variable that is not defined".into());
                } else if is_abs_path(&exp) && !exp.contains(';') {
                    d.paths.push(exp.clone());
                    if self.validator.exists(&exp) == Some(false) {
                        notes.push("Path not found".into());
                        d.bad_paths += 1;
                    }
                }
                if len > MAX_VALUE_LEN {
                    notes.push(format!(
                        "{} characters: Windows allows at most {}",
                        group_digits(len),
                        group_digits(MAX_VALUE_LEN)
                    ));
                }
                infos.push(VarInfo {
                    var: var.clone(),
                    is_list,
                    secret: self.is_secret(&var.name),
                    exp,
                    entries,
                    issue: (!notes.is_empty()).then(|| notes.join("\n")),
                    len,
                });
            }
            match scope {
                Scope::User => d.user = infos,
                Scope::System => d.system = infos,
            }
        }

        for u in &d.user {
            let Some(s) = d.system.iter().find(|s| ieq(&s.var.name, &u.var.name)) else { continue };
            let merged = u.var.name.eq_ignore_ascii_case("PATH");
            let eff = if merged {
                format!("{} entries, system first", s.entries.len() + u.entries.len())
            } else {
                u.var.value.clone()
            };
            d.conflicts.push(Conflict {
                name: u.var.name.clone(),
                merged,
                secret: u.secret || s.secret,
                sys: s.var.value.clone(),
                usr: u.var.value.clone(),
                eff,
            });
        }
        d.changes = diff(&self.saved, &self.work);
        d
    }

    // ───────────────────────────── editing ─────────────────────────────

    pub fn selected(&self, scope: Scope) -> Option<&str> {
        self.sel[scope_idx(scope)].as_deref()
    }

    pub fn select(&mut self, scope: Scope, name: Option<String>) {
        self.sel[scope_idx(scope)] = name;
        self.checked.clear();
        self.new_entry.clear();
        self.drag = None;
    }

    pub fn set_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.checked.clear();
            self.drag = None;
        }
    }

    pub fn delete_var(&mut self, scope: Scope, name: &str) {
        self.work.remove(scope, name);
        self.touch();
    }

    /// Change the entries of a list variable.
    pub fn edit_list(&mut self, scope: Scope, name: &str, f: impl FnOnce(&mut Vec<String>)) {
        if let Some(var) = self.work.find_mut(scope, name) {
            let mut items = split_list(&var.value);
            f(&mut items);
            var.value = join_list(&items);
            var.expand |= has_reference(&var.value);
            self.touch();
        }
    }

    /// Keep the checked positions pointing at the same entries after a reorder or removal.
    pub fn remap_checked(&mut self, f: impl Fn(usize) -> Option<usize>) {
        self.checked = self.checked.iter().filter_map(|&i| f(i)).collect();
    }

    /// Move entry `from` so it ends up at position `to`.
    pub fn move_entry(&mut self, scope: Scope, name: &str, from: usize, to: usize) {
        if from == to {
            return;
        }
        let mut moved = false;
        self.edit_list(scope, name, |items| {
            if from < items.len() && to < items.len() {
                let item = items.remove(from);
                items.insert(to, item);
                moved = true;
            }
        });
        if moved {
            self.remap_checked(|i| {
                Some(if i == from {
                    to
                } else if from < to && i > from && i <= to {
                    i - 1
                } else if to < from && i >= to && i < from {
                    i + 1
                } else {
                    i
                })
            });
        }
    }

    pub fn remove_entry(&mut self, scope: Scope, name: &str, index: usize) {
        self.edit_list(scope, name, |items| {
            if index < items.len() {
                items.remove(index);
            }
        });
        self.remap_checked(|i| match i.cmp(&index) {
            std::cmp::Ordering::Less => Some(i),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(i - 1),
        });
    }

    /// Remove every checked entry.
    pub fn remove_checked(&mut self, scope: Scope, name: &str) {
        let gone = std::mem::take(&mut self.checked);
        self.edit_list(scope, name, |items| {
            let mut i = 0;
            items.retain(|_| {
                i += 1;
                !gone.contains(&(i - 1))
            });
        });
    }

    pub fn discard(&mut self) {
        self.work = self.saved.clone();
        self.checked.clear();
        self.toast = None;
        self.touch();
    }

    pub fn show_toast(&mut self, ctx: &egui::Context, title: impl Into<String>, body: impl Into<String>) {
        self.set_toast(ctx, title, body, false);
    }

    pub fn show_error(&mut self, ctx: &egui::Context, title: impl Into<String>, body: impl Into<String>) {
        self.set_toast(ctx, title, body, true);
    }

    fn set_toast(&mut self, ctx: &egui::Context, title: impl Into<String>, body: impl Into<String>, error: bool) {
        let now = ctx.input(|i| i.time);
        self.toast = Some(Toast {
            title: title.into(),
            body: body.into(),
            error,
            until: now + if error { 12.0 } else { 7.0 },
        });
        ctx.request_repaint();
    }

    pub fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.pal = Palette::of(dark);
        theme::apply(ctx, &self.pal);
        self.settings.dark = Some(dark);
        self.settings.save();
    }

    // ───────────────────────────── registry sync ─────────────────────────────

    /// Pick up changes made outside Envoy, keeping pending edits on top.
    fn sync_external(&mut self) {
        let Ok(fresh) = winenv::read_all() else { return };
        if fresh != self.saved {
            let pending = diff(&self.saved, &self.work);
            self.work = rebase(&fresh, &pending);
            self.saved = fresh;
            self.checked.clear();
            self.touch();
        }
    }

    pub fn recheck_paths(&mut self) {
        self.validator.reset();
        self.touch();
    }

    pub fn applying(&self) -> bool {
        self.apply_rx.is_some()
    }

    /// Write pending changes to the registry on a worker thread (the UAC prompt blocks).
    pub fn start_apply(&mut self, ctx: &egui::Context) {
        if self.applying() || self.derived.changes.is_empty() {
            return;
        }
        let changes = self.derived.changes.clone();
        let elevated = self.elevated;
        let (tx, rx) = channel();
        self.apply_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(run_apply(changes, elevated));
            ctx.request_repaint();
        });
    }

    fn poll_apply(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.apply_rx else { return };
        let out = match rx.try_recv() {
            Ok(out) => out,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => {
                self.apply_rx = None;
                self.show_error(ctx, "Apply failed", "The worker stopped unexpectedly. Nothing else was changed.");
                return;
            }
        };
        self.apply_rx = None;
        if let Some(fresh) = out.fresh {
            self.work = rebase(&fresh, &out.failed);
            self.saved = fresh;
            self.checked.clear();
        }
        self.last_backup = store::list_backups().first().map(|b| b.backup.created);
        self.validator.reset();
        self.touch();

        if !out.errors.is_empty() {
            let title = if out.applied_any { "Some changes were not applied" } else { "Nothing was applied" };
            self.show_error(ctx, title, out.errors.join("\n"));
        } else if out.cancelled {
            let title = if out.applied_any {
                "User changes applied. System changes are still pending."
            } else {
                "System changes are still pending."
            };
            self.show_error(ctx, title, "Administrator approval was declined.");
        } else {
            self.show_toast(
                ctx,
                "Applied. Change broadcast sent to running apps.",
                "New terminals and apps see the update now. Terminals already open keep the old values until restarted.",
            );
        }
    }

    /// Restart through UAC, carrying pending edits along.
    pub fn relaunch_elevated(&mut self, ctx: &egui::Context) {
        let mut args: Vec<String> = vec!["--from-user".into(), self.user.clone()];
        let mut session = None;
        if !self.derived.changes.is_empty() {
            let file = temp_file("session");
            if serde_json::to_vec(&self.work).ok().and_then(|b| std::fs::write(&file, b).ok()).is_some() {
                args.push("--session".into());
                args.push(file.to_string_lossy().into_owned());
                session = Some(file);
            }
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        match winenv::run_self_elevated(&refs, false) {
            Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => {
                if let Some(file) = session {
                    let _ = std::fs::remove_file(file);
                }
                match e {
                    ElevateError::Cancelled => self.show_error(
                        ctx,
                        "Still running as a standard user",
                        "Administrator approval was declined.",
                    ),
                    ElevateError::Other(msg) => self.show_error(ctx, "Could not restart as administrator", msg),
                }
            }
        }
    }
}

fn temp_file(kind: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("envoy-{kind}-{}-{nanos}.json", std::process::id()))
}

fn run_apply(changes: Vec<Change>, elevated: bool) -> ApplyOutcome {
    let mut out =
        ApplyOutcome { applied_any: false, failed: Vec::new(), cancelled: false, errors: Vec::new(), fresh: None };

    // Never write without a snapshot to go back to.
    let n = changed_var_count(&changes);
    let note = format!("Before apply: {n} change{}", if n == 1 { "" } else { "s" });
    let backup = winenv::read_all()
        .map_err(|e| e.to_string())
        .and_then(|current| store::save_backup(&current, &note, "auto").map_err(|e| e.to_string()));
    if let Err(e) = backup {
        out.errors.push(format!("Could not save a backup first, so nothing was changed. ({e})"));
        out.failed = changes;
        return out;
    }

    let (user, system): (Vec<Change>, Vec<Change>) = changes.into_iter().partition(|c| c.scope == Scope::User);

    if !user.is_empty() {
        match winenv::apply_changes(&user) {
            Ok(()) => out.applied_any = true,
            Err(e) => {
                out.errors.push(e);
                out.failed.extend(user);
            }
        }
    }

    if !system.is_empty() {
        if elevated {
            match winenv::apply_changes(&system) {
                Ok(()) => out.applied_any = true,
                Err(e) => {
                    out.errors.push(e);
                    out.failed.extend(system);
                }
            }
        } else {
            // Hand just the system changes to an elevated copy of ourselves.
            let file = temp_file("apply");
            let written = serde_json::to_vec(&system)
                .map_err(|e| e.to_string())
                .and_then(|b| std::fs::write(&file, b).map_err(|e| e.to_string()));
            match written {
                Err(e) => {
                    out.errors.push(format!("Could not prepare the system changes: {e}"));
                    out.failed.extend(system);
                }
                Ok(()) => {
                    let path = file.to_string_lossy().into_owned();
                    match winenv::run_self_elevated(&["--apply-elevated", &path], true) {
                        Ok(Some(0)) => out.applied_any = true,
                        Ok(code) => {
                            let msg = std::fs::read_to_string(winenv::err_file(&file))
                                .unwrap_or_else(|_| format!("The elevated helper failed ({code:?})."));
                            out.errors.push(msg);
                            out.failed.extend(system);
                        }
                        Err(ElevateError::Cancelled) => {
                            out.cancelled = true;
                            out.failed.extend(system);
                        }
                        Err(ElevateError::Other(e)) => {
                            out.errors.push(format!("Could not ask for administrator approval: {e}"));
                            out.failed.extend(system);
                        }
                    }
                    let _ = std::fs::remove_file(winenv::err_file(&file));
                    let _ = std::fs::remove_file(&file);
                }
            }
        }
    }

    if out.applied_any {
        winenv::broadcast_change();
    }
    out.fresh = winenv::read_all().ok();
    out
}

/// 32767 → "32,767"
pub fn group_digits(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
