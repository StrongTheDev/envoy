//! Modal dialogs: edit / new variable, move entries, import / export, backups.

use crate::app::{App, Derived, VarInfo, group_digits};
use crate::model::*;
use crate::store::{self, BackupEntry};
use crate::theme::{self, Icon, Palette};
use crate::transfer::{self, Format};
use crate::widgets::*;
use crate::winenv;
use egui::{Align, Frame, Id, Layout, Margin, RichText, ScrollArea, Stroke, Ui, vec2};

const WIDTH: f32 = 512.0;

#[derive(Default)]
pub enum Dialog {
    #[default]
    None,
    Edit(EditDlg),
    Move(MoveDlg),
    Io(IoDlg),
    Backups(BackupsDlg),
}

pub struct EditDlg {
    /// The variable being edited; `None` for a new one.
    orig: Option<(Scope, String)>,
    orig_expand: bool,
    name: String,
    scope: Scope,
    list: bool,
    secret: bool,
    /// The value, or one entry per line for lists.
    text: String,
    focus_name: bool,
}

pub struct MoveDlg {
    scope: Scope,
    source: String,
    name: String,
    add_ref: bool,
    expand_refs: bool,
    focus_name: bool,
}

pub struct IoDlg {
    import: bool,
    user: bool,
    system: bool,
    format: Format,
    path: String,
    /// What `preview` was loaded from.
    loaded: Option<(String, Format)>,
    preview: Option<Result<EnvSet, String>>,
}

pub struct BackupsDlg {
    list: Vec<BackupEntry>,
    note: String,
}

#[derive(Clone, Copy, PartialEq)]
enum ImportStatus {
    New,
    Changed,
    Same,
}

impl App {
    // ───────────────────────────── opening ─────────────────────────────

    pub fn open_edit(&mut self, scope: Scope, v: Option<&VarInfo>) {
        self.dialog = Dialog::Edit(match v {
            Some(v) => EditDlg {
                orig: Some((scope, v.var.name.clone())),
                orig_expand: v.var.expand,
                name: v.var.name.clone(),
                scope,
                list: v.is_list,
                secret: v.secret,
                text: if v.is_list { split_list(&v.var.value).join("\n") } else { v.var.value.clone() },
                focus_name: false,
            },
            None => EditDlg {
                orig: None,
                orig_expand: false,
                name: String::new(),
                scope,
                list: false,
                secret: false,
                text: String::new(),
                focus_name: true,
            },
        });
    }

    pub fn open_move(&mut self, scope: Scope, source: &str) {
        self.dialog = Dialog::Move(MoveDlg {
            scope,
            source: source.to_owned(),
            name: String::new(),
            add_ref: true,
            expand_refs: true,
            focus_name: true,
        });
    }

    pub fn open_io(&mut self) {
        let docs = std::env::var_os("USERPROFILE")
            .map(|h| std::path::PathBuf::from(h).join("Documents"))
            .filter(|d| d.is_dir())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        self.dialog = Dialog::Io(IoDlg {
            import: false,
            user: true,
            system: true,
            format: Format::Json,
            path: docs.join("envoy-export.json").to_string_lossy().into_owned(),
            loaded: None,
            preview: None,
        });
    }

    pub fn open_backups(&mut self) {
        self.dialog = Dialog::Backups(BackupsDlg { list: store::list_backups(), note: String::new() });
    }

    // ───────────────────────────── dispatch ─────────────────────────────

    pub fn dialogs(&mut self, ctx: &egui::Context, d: &Derived) {
        let mut dlg = std::mem::take(&mut self.dialog);
        if matches!(dlg, Dialog::None) {
            return;
        }
        let p = self.pal;
        let frame = Frame::new()
            .fill(p.surface)
            .inner_margin(Margin::same(24))
            .shadow(ctx.global_style().visuals.window_shadow);
        let backdrop = egui::Color32::from_rgba_unmultiplied(0x2d, 0x2b, 0x2b, if p.dark { 170 } else { 128 });
        let modal = egui::Modal::new(Id::unique("envoy-dialog")).frame(frame).backdrop_color(backdrop).show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            match &mut dlg {
                Dialog::None => true,
                Dialog::Edit(s) => self.edit_dialog(ui, s),
                Dialog::Move(s) => self.move_dialog(ui, d, s),
                Dialog::Io(s) => self.io_dialog(ui, s),
                Dialog::Backups(s) => self.backups_dialog(ui, s),
            }
        });
        // Escape closes; a stray click on the backdrop should not throw away what was typed.
        let dismissed = modal.should_close() && !modal.backdrop_response.clicked();
        let keep = modal.inner && !dismissed;
        // A dialog may have opened another one; that one wins.
        if keep && matches!(self.dialog, Dialog::None) {
            self.dialog = dlg;
        }
    }

    // ───────────────────────────── edit / new ─────────────────────────────

    /// Each dialog returns false to close.
    fn edit_dialog(&mut self, ui: &mut Ui, s: &mut EditDlg) -> bool {
        let p = self.pal;
        let mut keep = true;
        title(ui, &p, if s.orig.is_some() { "Edit variable" } else { "New variable" });

        field_label(ui, &p, "Name");
        let name_field = text_input(ui, &p, &mut s.name, "", WIDTH, theme::mono(14.0));
        if std::mem::take(&mut s.focus_name) {
            name_field.request_focus();
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            ui.vertical(|ui| {
                field_label(ui, &p, "Scope");
                if let Some(i) = seg(ui, &p, "scope", &["User", "System"], s.scope as usize) {
                    s.scope = Scope::ALL[i];
                }
            });
            ui.vertical(|ui| {
                field_label(ui, &p, "Type");
                if let Some(i) = seg(ui, &p, "type", &["Text", "List"], s.list as usize) {
                    let to_list = i == 1;
                    if to_list != s.list {
                        s.text = if to_list { split_list(&s.text).join("\n") } else { join_list(&lines(&s.text)) };
                        s.list = to_list;
                    }
                }
            });
            ui.vertical(|ui| {
                field_label(ui, &p, "Display");
                ui.add_space(8.0);
                let text = spans(&[("Secret (mask the value)", theme::body(13.0), p.text)]);
                check_label(ui, &p, &mut s.secret, text);
            });
        });

        ui.add_space(4.0);
        field_label(ui, &p, if s.list { "Entries (one per line)" } else { "Value" });
        ScrollArea::vertical().id_salt("edit-value").max_height(150.0).show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut s.text)
                    .font(theme::mono(13.0))
                    .text_color(p.text)
                    .margin(Margin::symmetric(10, 9))
                    .desired_rows(6)
                    .desired_width(WIDTH),
            );
        });

        let name = s.name.trim().to_owned();
        let value = if s.list { join_list(&lines(&s.text)) } else { s.text.replace(['\r', '\n'], "") };
        let expand = has_reference(&value) || s.orig_expand;
        let len = value.encode_utf16().count();

        ui.add_space(4.0);
        ui.label(caps("Expanded preview", 11.0, p.text));
        ui.add_space(-4.0);
        Frame::new().fill(p.bg).stroke(Stroke::new(1.0, p.divider)).inner_margin(Margin::same(10)).show(ui, |ui| {
            ui.set_width(WIDTH - 22.0);
            ui.set_min_height(22.0);
            ScrollArea::vertical().id_salt("edit-preview").max_height(86.0).show(ui, |ui| {
                let ex = Expander::new(&self.work);
                let show = |v: &str| if expand { ex.expand(s.scope, v) } else { v.to_owned() };
                let preview =
                    if s.list { lines(&s.text).iter().map(|l| show(l)).collect::<Vec<_>>().join("\n") } else { show(&value) };
                ui.add(egui::Label::new(RichText::new(preview).font(theme::mono(12.5)).color(p.a700)).wrap());
            });
        });

        // Notes under the preview.
        let over = len > MAX_VALUE_LEN;
        let mut note = format!("{} of {} characters", group_digits(len), group_digits(MAX_VALUE_LEN));
        if over {
            note.push_str(" — too long for Windows to store");
        } else if len > NATIVE_EDITOR_LIMIT {
            note.push_str(&format!(
                " · past the {} the built-in Windows editor can handle",
                group_digits(NATIVE_EDITOR_LIMIT)
            ));
        }
        label(ui, note, theme::body(12.0), if over { p.a700 } else { p.muted(0.7) });

        let is_self = |n: &str| s.orig.as_ref().is_some_and(|(sc, on)| *sc == s.scope && ieq(on, n));
        let replaces = !name.is_empty() && !is_self(&name) && self.work.find(s.scope, &name).is_some();
        let bad_name = name.contains('=');
        if bad_name {
            label(ui, "A name cannot contain \"=\".", theme::body(12.0), p.a700);
        } else if replaces {
            label(
                ui,
                format!("Replaces the existing {} variable {name}.", s.scope.lower()),
                theme::body(12.0),
                p.a700,
            );
        }
        if s.scope == Scope::System && !self.elevated {
            ui.horizontal(|ui| {
                icon(ui, Icon::Shield, 16.0, p.text);
                label(
                    ui,
                    "Saving to system scope needs administrator approval when you apply.",
                    theme::body(12.0),
                    p.text,
                );
            });
        }

        let valid = !name.is_empty() && !bad_name && !over;
        actions(ui, |ui| {
            if Btn::new("Save to pending changes", BtnStyle::primary(&p)).enabled(valid).show(ui).clicked() {
                self.save_edit(s, Var { name: name.clone(), value: value.clone(), expand });
                keep = false;
            }
            if Btn::new("Cancel", BtnStyle::secondary(&p)).show(ui).clicked() {
                keep = false;
            }
        });
        keep
    }

    fn save_edit(&mut self, s: &EditDlg, var: Var) {
        let name = var.name.clone();
        let key = name.to_lowercase();
        match self.settings.list.get(&key) {
            _ if s.list == looks_like_list(&name, &var.value) => {
                self.settings.list.remove(&key);
            }
            _ => {
                self.settings.list.insert(key.clone(), s.list);
            }
        }
        if s.secret == looks_secret(&name) {
            self.settings.secret.remove(&key);
        } else {
            self.settings.secret.insert(key, s.secret);
        }
        self.settings.save();
        self.checked.clear();

        match &s.orig {
            Some((old_scope, old_name)) if *old_scope == s.scope => {
                if !ieq(old_name, &name) {
                    self.work.remove(s.scope, &name); // renamed onto another variable
                }
                match self.work.find_mut(s.scope, old_name) {
                    Some(slot) => *slot = var,
                    None => self.work.set(s.scope, var),
                }
            }
            Some((old_scope, old_name)) => {
                self.work.remove(*old_scope, old_name);
                self.work.set(s.scope, var);
            }
            None => self.work.set(s.scope, var),
        }

        let was_open = s.orig.as_ref().is_some_and(|(sc, n)| self.selected(*sc).is_some_and(|sel| ieq(sel, n)));
        if s.list && (was_open || s.orig.is_none()) {
            self.select(s.scope, Some(name));
        }
        self.touch();
    }

    // ───────────────────────────── move entries ─────────────────────────────

    fn move_dialog(&mut self, ui: &mut Ui, d: &Derived, s: &mut MoveDlg) -> bool {
        let p = self.pal;
        let Some(src) = d.find(s.scope, &s.source) else { return false };
        let moved: Vec<&crate::app::EntryInfo> =
            src.entries.iter().enumerate().filter(|(i, _)| self.checked.contains(i)).map(|(_, e)| e).collect();
        if moved.is_empty() {
            return false;
        }
        let mut keep = true;
        let n = moved.len();
        title(ui, &p, &format!("Move {n} {} to a new variable", if n == 1 { "entry" } else { "entries" }));

        field_label(ui, &p, "New variable name");
        let field = text_input(ui, &p, &mut s.name, "e.g. PATH_TOOLS", WIDTH, theme::mono(14.0));
        if std::mem::take(&mut s.focus_name) {
            field.request_focus();
        }
        if field.changed() {
            s.name = s
                .name
                .to_uppercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
                .collect();
        }
        let new_name = s.name.trim().to_owned();
        let shown = if new_name.is_empty() { "NEW_VARIABLE" } else { new_name.as_str() };
        let has_refs = moved.iter().any(|e| e.exp != e.raw);
        let remain = src.entries.len() - n + s.add_ref as usize;

        ui.add_space(4.0);
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            ui.label(caps(&format!("{shown} · new"), 11.0, p.text));
            rule(ui, &p, 2.0, p.divider);
            ScrollArea::vertical().id_salt("moved").max_height(132.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for e in &moved {
                    let text = if s.expand_refs && has_refs { &e.exp } else { &e.raw };
                    ui.add(egui::Label::new(RichText::new(text).font(theme::mono(11.5)).color(p.text)).wrap());
                    rule(ui, &p, 1.0, p.n300);
                }
            });
            let ui = &mut cols[1];
            ui.label(caps(&format!("{} · keeps {remain}", s.source), 11.0, p.text));
            rule(ui, &p, 2.0, p.divider);
            ui.add(
                egui::Label::new(
                    RichText::new("Remaining entries stay in order.").font(theme::body(12.5)).color(p.muted(0.8)),
                )
                .wrap(),
            );
        });

        ui.add_space(4.0);
        check_label(
            ui,
            &p,
            &mut s.add_ref,
            spans(&[
                ("Add ", theme::body(13.0), p.text),
                (&format!("%{shown}%"), theme::mono_bold(13.0), p.text),
                (&format!(" to {} so lookups keep working", s.source), theme::body(13.0), p.text),
            ]),
        );
        if has_refs {
            check_label(
                ui,
                &p,
                &mut s.expand_refs,
                spans(&[("Store the expanded paths in the new variable", theme::body(13.0), p.text)]),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Recommended: Windows does not reliably resolve a %VAR% inside a variable that is itself \
                         referenced from another one.",
                    )
                    .font(theme::body(12.0))
                    .color(p.muted(0.7)),
                )
                .wrap(),
            );
        }

        let exists = !new_name.is_empty() && self.work.find(s.scope, &new_name).is_some();
        if exists {
            label(
                ui,
                format!("A {} variable named {new_name} already exists.", s.scope.lower()),
                theme::body(12.0),
                p.a700,
            );
        }

        actions(ui, |ui| {
            let ok = !new_name.is_empty() && !exists;
            if Btn::new("Create variable", BtnStyle::primary(&p)).enabled(ok).show(ui).clicked() {
                let use_expanded = s.expand_refs && has_refs;
                let values: Vec<String> =
                    moved.iter().map(|e| if use_expanded { e.exp.clone() } else { e.raw.clone() }).collect();
                let first = self.checked.iter().copied().min().unwrap_or(0);
                let gone = std::mem::take(&mut self.checked);
                let add_ref = s.add_ref.then(|| format!("%{new_name}%"));
                self.edit_list(s.scope, &s.source, |items| {
                    let mut i = 0;
                    items.retain(|_| {
                        i += 1;
                        !gone.contains(&(i - 1))
                    });
                    if let Some(reference) = add_ref {
                        // Where the first moved entry was, so lookup order is unchanged.
                        items.insert(first.min(items.len()), reference);
                    }
                });
                let value = join_list(&values);
                if !looks_like_list(&new_name, &value) {
                    self.settings.list.insert(new_name.to_lowercase(), true);
                    self.settings.save();
                }
                self.work.set(s.scope, Var::new(new_name.clone(), value));
                self.touch();
                self.show_toast(
                    ui.ctx(),
                    format!("Moved {n} {} to {new_name}", if n == 1 { "entry" } else { "entries" }),
                    "Pending until you apply changes.",
                );
                keep = false;
            }
            if Btn::new("Cancel", BtnStyle::secondary(&p)).show(ui).clicked() {
                keep = false;
            }
        });
        keep
    }

    // ───────────────────────────── import / export ─────────────────────────────

    fn io_dialog(&mut self, ui: &mut Ui, s: &mut IoDlg) -> bool {
        let p = self.pal;
        let mut keep = true;
        title(ui, &p, "Import / Export");

        if let Some(i) = seg(ui, &p, "io-mode", &["Export", "Import"], s.import as usize) {
            s.import = i == 1;
            if s.import && !s.format.can_import() {
                s.format = Format::Json;
            }
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            let user = format!("User ({})", self.work.user.len());
            let system = format!("System ({})", self.work.system.len());
            check_label(ui, &p, &mut s.user, spans(&[(&user, theme::body(14.0), p.text)]));
            check_label(ui, &p, &mut s.system, spans(&[(&system, theme::body(14.0), p.text)]));
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.vertical(|ui| {
                field_label(ui, &p, "Format");
                let before = s.format;
                egui::ComboBox::from_id_salt("io-format").width(140.0).selected_text(s.format.label()).show_ui(ui, |ui| {
                    for f in Format::ALL {
                        if !s.import || f.can_import() {
                            ui.selectable_value(&mut s.format, f, f.label());
                        }
                    }
                });
                if s.format != before && !s.import {
                    // Keep the file name, swap the extension.
                    let stem = s.path.strip_suffix(&format!(".{}", before.ext())).unwrap_or(&s.path).to_owned();
                    s.path = format!("{stem}.{}", s.format.ext());
                }
            });
            ui.vertical(|ui| {
                field_label(ui, &p, "File");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let w = ui.available_width() - 8.0 - 92.0;
                    text_input(ui, &p, &mut s.path, r"C:\path\to\file", w, theme::mono(12.5));
                    if Btn::new("Browse…", BtnStyle::secondary(&p)).show(ui).clicked() {
                        browse(s);
                    }
                });
            });
        });

        // Import preview: what the file would change.
        let mut staged: Vec<(Scope, Var, ImportStatus)> = Vec::new();
        if s.import {
            if let Some(f) = Format::from_path(&s.path).filter(|f| f.can_import()) {
                s.format = f;
            }
            let key = (s.path.clone(), s.format);
            if s.loaded.as_ref() != Some(&key) {
                s.preview = (!s.path.trim().is_empty()).then(|| {
                    std::fs::read(s.path.trim())
                        .map_err(|e| format!("Cannot read the file: {e}"))
                        .and_then(|bytes| transfer::import(&bytes, s.format))
                });
                s.loaded = Some(key);
            }
            ui.add_space(4.0);
            ui.label(caps("Preview of file", 11.0, p.text));
            rule(ui, &p, 2.0, p.divider);
            match &s.preview {
                None => {
                    label(ui, "Choose a .json, .env or .reg file to import.", theme::body(13.0), p.muted(0.7));
                }
                Some(Err(e)) => {
                    ui.add(egui::Label::new(RichText::new(e).font(theme::body(13.0)).color(p.a700)).wrap());
                }
                Some(Ok(env)) => {
                    for scope in Scope::ALL {
                        let wanted = if scope == Scope::User { s.user } else { s.system };
                        if !wanted {
                            continue;
                        }
                        for var in env.get(scope) {
                            let status = match self.work.find(scope, &var.name) {
                                None => ImportStatus::New,
                                Some(w) if w.value == var.value && w.expand == var.expand => ImportStatus::Same,
                                Some(_) => ImportStatus::Changed,
                            };
                            staged.push((scope, var.clone(), status));
                        }
                    }
                    if staged.is_empty() {
                        label(ui, "No variables for the selected scopes in this file.", theme::body(13.0), p.muted(0.7));
                    }
                    ScrollArea::vertical().id_salt("io-preview").max_height(168.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        for (scope, var, status) in &staged {
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(vec2(74.0, 20.0), Layout::left_to_right(Align::Center), |ui| {
                                    match status {
                                        ImportStatus::New => tag(ui, &p, "new", TagKind::Accent),
                                        ImportStatus::Changed => tag(ui, &p, "changed", TagKind::Neutral),
                                        ImportStatus::Same => tag(ui, &p, "same", TagKind::Outline),
                                    };
                                    ui.add_space(ui.available_width());
                                });
                                label(ui, &var.name, theme::mono(12.5), p.text);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.add_space(12.0);
                                    label(ui, scope.lower(), theme::mono(12.5), p.muted(0.7));
                                });
                            });
                        }
                    });
                }
            }
        }

        ui.add_space(4.0);
        let hint = if s.import {
            "Imports are staged as pending changes. A backup is saved automatically when you apply them."
        } else {
            "Exports include pending changes. Values are written as plain text, secrets included."
        };
        ui.add(egui::Label::new(RichText::new(hint).font(theme::body(12.0)).color(p.muted(0.75))).wrap());

        let changes = staged.iter().filter(|(_, _, st)| *st != ImportStatus::Same).count();
        let export_n = if s.user { self.work.user.len() } else { 0 } + if s.system { self.work.system.len() } else { 0 };
        let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        let (text, enabled) = if s.import {
            (format!("Import {}", plural(changes, "change")), changes > 0)
        } else {
            (format!("Export {}", plural(export_n, "variable")), export_n > 0 && !s.path.trim().is_empty())
        };
        actions(ui, |ui| {
            if Btn::new(&text, BtnStyle::primary(&p)).enabled(enabled).show(ui).clicked() {
                if s.import {
                    for (scope, var, status) in staged.drain(..) {
                        if status != ImportStatus::Same {
                            self.work.set(scope, var);
                        }
                    }
                    self.checked.clear();

                    self.touch();
                    self.show_toast(
                        ui.ctx(),
                        "Import staged",
                        format!("{} added to pending changes.", plural(changes, "change")),
                    );
                    keep = false;
                } else {
                    let bytes = transfer::export(&self.work, s.user, s.system, s.format);
                    match std::fs::write(s.path.trim(), bytes) {
                        Ok(()) => {
                            self.show_toast(ui.ctx(), "Exported", format!("Saved to {}", s.path.trim()));
                            keep = false;
                        }
                        Err(e) => self.show_error(ui.ctx(), "Export failed", e.to_string()),
                    }
                }
            }
            if Btn::new("Cancel", BtnStyle::secondary(&p)).show(ui).clicked() {
                keep = false;
            }
        });
        keep
    }

    // ───────────────────────────── backups ─────────────────────────────

    fn backups_dialog(&mut self, ui: &mut Ui, s: &mut BackupsDlg) -> bool {
        let p = self.pal;
        let mut keep = true;
        title(ui, &p, "Backups");
        ui.add(
            egui::Label::new(
                RichText::new(
                    "A snapshot of both scopes is saved before every apply. Restoring stages the snapshot as pending \
                     changes, so nothing is written until you apply.",
                )
                .font(theme::body(13.0))
                .color(p.muted(0.8)),
            )
            .wrap(),
        );

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let w = ui.available_width() - 8.0 - 124.0;
            text_input(ui, &p, &mut s.note, "Note for a manual backup, e.g. pre-docker-install", w, theme::body(13.0));
            if Btn::new("Back up now", BtnStyle::secondary(&p)).show(ui).clicked() {
                let note = if s.note.trim().is_empty() { "Manual backup" } else { s.note.trim() };
                let result = winenv::read_all()
                    .map_err(|e| e.to_string())
                    .and_then(|env| store::save_backup(&env, note, "manual").map_err(|e| e.to_string()));
                match result {
                    Ok(_) => {
                        s.note.clear();
                        s.list = store::list_backups();
                        self.last_backup = s.list.first().map(|b| b.backup.created);
                    }
                    Err(e) => self.show_error(ui.ctx(), "Backup failed", e),
                }
            }
        });

        ui.add_space(4.0);
        rule(ui, &p, 2.0, p.divider);
        if s.list.is_empty() {
            ui.add_space(8.0);
            label(ui, "No backups yet.", theme::body(13.0), p.muted(0.7));
            ui.add_space(8.0);
        }
        let mut delete: Option<usize> = None;
        ScrollArea::vertical().id_salt("backups").max_height(300.0).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, b) in s.list.iter().enumerate() {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(12.0, 2.0);
                    let left = ui.available_width() - 224.0;
                    ui.allocate_ui_with_layout(vec2(left, 40.0), Layout::top_down(Align::Min), |ui| {
                        let when = store::friendly_time(&b.backup.created);
                        ui.add(egui::Label::new(RichText::new(when).font(theme::semi(14.0)).color(p.text)).truncate());
                        let detail = format!("{} · {} variables", b.backup.note, b.backup.env.total());
                        ui.add(
                            egui::Label::new(RichText::new(detail).font(theme::body(12.0)).color(p.muted(0.75))).truncate(),
                        );
                        ui.add_space(ui.available_width().min(0.0));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.add_space(10.0);
                        if icon_btn(ui, Icon::Trash, 32.0, p.text).on_hover_text("Delete this backup").clicked() {
                            delete = Some(i);
                        }
                        if Btn::new("Restore", BtnStyle::secondary(&p)).small().show(ui).clicked() {
                            self.work = b.backup.env.clone();
                            self.checked.clear();
                            self.touch();
                            self.show_toast(
                                ui.ctx(),
                                format!("Restore staged: {}", store::friendly_time(&b.backup.created)),
                                "Review and apply to write it to Windows. A new backup is taken first.",
                            );
                            keep = false;
                        }
                        ui.add_space(6.0);
                        tag(ui, &p, &b.backup.kind, TagKind::Neutral);
                    });
                });
                ui.add_space(10.0);
                rule(ui, &p, 1.0, p.n300);
            }
        });
        if let Some(i) = delete {
            let _ = store::delete_backup(&s.list[i]);
            s.list.remove(i);
            self.last_backup = s.list.first().map(|b| b.backup.created);
        }

        actions(ui, |ui| {
            if Btn::new("Close", BtnStyle::secondary(&p)).show(ui).clicked() {
                keep = false;
            }
        });
        keep
    }
}

// ───────────────────────────── helpers ─────────────────────────────

fn title(ui: &mut Ui, p: &Palette, text: &str) {
    label(ui, text, theme::heavy(20.0), p.text);
    ui.add_space(4.0);
}

/// Right-aligned button row at the bottom of a dialog (add buttons right to left).
fn actions(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(8.0);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 36.0), Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        add(ui);
    });
}

fn rule(ui: &mut Ui, _p: &Palette, thickness: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), thickness), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, color);
}

/// Non-empty entries of the list text box; `;` also separates.
fn lines(text: &str) -> Vec<String> {
    text.split(['\n', '\r', ';']).map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

fn browse(s: &mut IoDlg) {
    let mut dialog = rfd::FileDialog::new();
    let current = std::path::Path::new(s.path.trim());
    if let Some(dir) = current.parent().filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    if s.import {
        let picked = dialog
            .add_filter("Environment files", &["json", "env", "reg"])
            .add_filter("All files", &["*"])
            .pick_file();
        if let Some(path) = picked {
            s.path = path.to_string_lossy().into_owned();
            s.loaded = None;
        }
    } else {
        if let Some(name) = current.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
        if let Some(path) = dialog.add_filter(s.format.label(), &[s.format.ext()]).save_file() {
            s.path = path.to_string_lossy().into_owned();
        }
    }
}
