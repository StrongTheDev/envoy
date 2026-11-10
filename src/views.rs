//! The main window: tab strip, toolbar, variable table, conflicts, list editor, bars.

use crate::app::{App, Conflict, Derived, Filter, Tab, VarInfo, group_digits};
use crate::model::{MAX_VALUE_LEN, Scope, changed_var_count, ieq};
use crate::store;
use crate::theme::{self, Icon, Palette};
use crate::widgets::*;
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align, Align2, Area, CursorIcon, Frame, Id, Layout, Margin, Order, Rect, RichText, ScrollArea,
    Sense, Stroke, Ui, Vec2, pos2, vec2,
};

const MASK: &str = "••••••••••••••••";
const NAME_COL: f32 = 230.0;
const ACTIONS_COL: f32 = 112.0;
const ROW_H: f32 = 64.0;

impl App {
    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.tick(&ctx);
        let d = self.derived.clone();
        let p = self.pal;

        let mut r = ui.max_rect();
        ui.painter().rect_filled(r, 0.0, p.bg);
        let tabs = cut_top(&mut r, 48.0);
        let toolbar = cut_top(&mut r, 60.0);
        let status = cut_bottom(&mut r, 32.0);
        let dirty = (!d.changes.is_empty()).then(|| cut_bottom(&mut r, 54.0));

        let scope = self.tab.scope();
        let open_list = if self.tab == Tab::Conflicts {
            None
        } else {
            self.selected(scope).and_then(|n| d.find(scope, n)).filter(|v| v.is_list)
        };
        let pane_w = 440f32.min(r.width() * 0.5);
        let pane = open_list.map(|v| (cut_right(&mut r, pane_w), v));

        region(ui, tabs, |ui| self.tabs_bar(ui, &d));
        region(ui, toolbar, |ui| self.toolbar(ui));
        region(ui, r, |ui| match self.tab {
            Tab::Conflicts => self.conflicts_view(ui, &d),
            _ => self.var_table(ui, &d, scope),
        });
        if let Some((rect, v)) = pane {
            region(ui, rect, |ui| self.list_pane(ui, scope, v));
        }
        if let Some(rect) = dirty {
            region(ui, rect, |ui| self.dirty_bar(ui, &d));
        }
        region(ui, status, |ui| self.status_bar(ui, &d));

        // Keep the toast clear of the bars stacked at the bottom right.
        let pane_bar = pane.is_some() && !self.checked.is_empty();
        let lift = if dirty.is_some() { 54.0 } else { 0.0 } + if pane_bar { 58.0 } else { 0.0 };
        self.toast_ui(&ctx, lift);
        self.dialogs(&ctx, &d);
    }

    // ───────────────────────────── tab strip ─────────────────────────────

    fn tabs_bar(&mut self, ui: &mut Ui, d: &Derived) {
        let p = self.pal;
        let rect = ui.max_rect();
        ui.painter().hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(2.0, p.divider));

        let tabs = [
            (Tab::User, "User", Icon::User, d.user.len()),
            (Tab::System, "System", Icon::Shield, d.system.len()),
            (Tab::Conflicts, "Conflicts", Icon::Conflict, d.conflicts.len()),
        ];
        let mut x = rect.left() + 8.0;
        for (tab, text, icon, count) in tabs {
            let active = self.tab == tab;
            let fg = if active { p.text } else { p.muted(0.72) };
            let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::semi(14.0), fg);
            let count = count.to_string();
            let count_w = ui.painter().layout_no_wrap(count.clone(), theme::body(11.0), fg).size().x + 20.0;
            let w = (20.0 + 16.0 + 8.0 + galley.size().x + 8.0 + count_w + 20.0).ceil();
            let r = Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
            let resp = ui.interact(r, ui.scope_id().with(("tab", text)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            if resp.hovered() && !active {
                ui.painter().rect_filled(r.with_max_y(r.bottom() - 2.0), 0.0, p.surface);
            }
            if active {
                ui.painter().rect_filled(r.with_min_y(r.bottom() - 4.0), 0.0, p.accent);
            }
            let cy = r.center().y - 1.0;
            let mut cx = r.left() + 20.0;
            paint_icon(ui, icon, pos2(cx + 8.0, cy), 16.0, fg);
            cx += 24.0;
            ui.painter().galley(pos2(cx, cy - galley.size().y / 2.0), galley.clone(), fg);
            cx += galley.size().x + 8.0;
            paint_tag(ui, &p, pos2(cx, cy - 10.0), &count, TagKind::Neutral);
            if resp.clicked() {
                self.set_tab(tab);
            }
            x += w;
        }

        let right = Rect::from_min_max(pos2(x, rect.top()), pos2(rect.right() - 12.0, rect.bottom() - 2.0));
        region(ui, right, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let (toggle, tip) =
                    if p.dark { (Icon::Sun, "Switch to light mode") } else { (Icon::Moon, "Switch to dark mode") };
                if icon_btn(ui, toggle, 32.0, p.text).on_hover_text(tip).clicked() {
                    self.set_dark(ui.ctx(), !p.dark);
                }
                if !self.elevated
                    && Btn::new("Run as administrator", BtnStyle::secondary(&p))
                        .tiny()
                        .show(ui)
                        .on_hover_text("Restart Envoy elevated. Pending changes are carried over.")
                        .clicked()
                {
                    self.relaunch_elevated(ui.ctx());
                }
                let (role, state) =
                    if self.elevated { ("Administrator ", " · elevated") } else { ("Standard user ", " · not elevated") };
                ui.label(spans(&[
                    (role, theme::body(12.0), p.text),
                    (&self.user, theme::semi(12.0), p.text),
                    (state, theme::body(12.0), p.text),
                ]));
                icon(ui, Icon::Shield, 16.0, p.text);
            });
        });
    }

    // ───────────────────────────── toolbar ─────────────────────────────

    fn toolbar(&mut self, ui: &mut Ui) {
        let p = self.pal;
        let rect = ui.max_rect();
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.divider));
        region(ui, rect.shrink2(vec2(16.0, 0.0)), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text(RichText::new("Search names and values").color(p.muted(0.5)))
                        .font(theme::body(14.0))
                        .margin(Margin { left: 34, right: 10, top: 9, bottom: 9 })
                        .desired_width(300.0),
                );
                paint_icon(ui, Icon::Search, pos2(search.rect.left() + 18.0, search.rect.center().y), 16.0, p.text);

                let filters = [Filter::All, Filter::Lists, Filter::Secrets, Filter::Issues];
                let current = filters.iter().position(|f| *f == self.filter).unwrap_or(0);
                if let Some(i) = seg(ui, &p, "filter", &["All", "Lists", "Secrets", "Issues"], current) {
                    self.filter = filters[i];
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if Btn::new("New variable", BtnStyle::primary(&p)).icon(Icon::Plus).show(ui).clicked() {
                        self.open_edit(self.tab.scope(), None);
                    }
                    if Btn::new("Import / Export", BtnStyle::secondary(&p)).icon(Icon::Transfer).show(ui).clicked() {
                        self.open_io();
                    }
                    if Btn::new("Backups", BtnStyle::secondary(&p)).icon(Icon::History).show(ui).clicked() {
                        self.open_backups();
                    }
                });
            });
        });
    }

    // ───────────────────────────── variable table ─────────────────────────────

    fn var_table(&mut self, ui: &mut Ui, d: &Derived, scope: Scope) {
        let p = self.pal;
        let mut r = ui.max_rect();

        if let Some(err) = self.load_error.clone() {
            banner(ui, &p, &mut r, Icon::Warn, "Could not read the registry. ", &err);
        }
        if scope == Scope::System {
            let rest = if self.elevated {
                "Variables here apply to every user on this PC. Envoy is running as administrator, so changes apply directly."
            } else {
                "Variables here apply to every user on this PC. Applying changes asks for administrator approval (UAC)."
            };
            banner(ui, &p, &mut r, Icon::Shield, "Machine scope. ", rest);
        } else if let Some(from) = self.foreign_user.clone() {
            let rest = format!(
                "Envoy was elevated with the account \"{}\", so these are that account's user variables, not {from}'s. \
                 To edit your own, run Envoy without elevation.",
                self.user
            );
            banner(ui, &p, &mut r, Icon::Warn, "Different account. ", &rest);
        }

        let header = cut_top(&mut r, 34.0);
        let painter = ui.painter();
        let y = header.center().y;
        let name = painter.layout_job(caps("Name", 11.0, p.text));
        painter.galley(pos2(header.left() + 16.0, y - name.size().y / 2.0), name, p.text);
        let mut value = caps("Value", 11.0, p.text);
        value.append(" · expanded in red", 0.0, TextFormat::simple(theme::body(11.0), p.a700));
        let value = painter.layout_job(value);
        painter.galley(pos2(header.left() + 16.0 + NAME_COL, y - value.size().y / 2.0), value, p.text);
        painter.hline(header.x_range(), header.bottom() - 1.0, Stroke::new(2.0, p.divider));

        let q = self.query.trim().to_lowercase();
        let rows: Vec<&VarInfo> = d
            .vars(scope)
            .iter()
            .filter(|v| {
                let masked = v.secret && !self.reveal.contains(&(scope, v.var.name.clone()));
                let hit = q.is_empty()
                    || v.var.name.to_lowercase().contains(&q)
                    || (!masked && v.var.value.to_lowercase().contains(&q));
                hit && match self.filter {
                    Filter::All => true,
                    Filter::Lists => v.is_list,
                    Filter::Secrets => v.secret,
                    Filter::Issues => v.issue.is_some(),
                }
            })
            .collect();

        region(ui, r, |ui| {
            ScrollArea::vertical().id_salt(("vars", scope)).auto_shrink(false).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if rows.is_empty() {
                    ui.add_space(48.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        label(ui, "No variables match.", theme::body(14.0), p.muted(0.7));
                    });
                }
                for v in rows {
                    self.var_row(ui, scope, v);
                }
            });
        });
    }

    fn var_row(&mut self, ui: &mut Ui, scope: Scope, v: &VarInfo) {
        let p = self.pal;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let name = &v.var.name;
        let id = ui.scope_id().with((scope, name.as_str()));
        let selected = v.is_list && self.selected(scope).is_some_and(|s| ieq(s, name));
        if selected {
            ui.painter().rect_filled(rect, 0.0, p.a100);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 0.0, p.surface);
        }
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.n300));

        // Name, warning, tags.
        let x = rect.left() + 16.0;
        let top = rect.top() + 10.0;
        let room = NAME_COL - 12.0 - if v.issue.is_some() { 24.0 } else { 0.0 };
        let galley = elided(ui, name, theme::mono_bold(13.5), p.text, room);
        let name_size = galley.size();
        ui.painter().galley(pos2(x, top), galley, p.text);
        if let Some(issue) = &v.issue {
            let c = pos2(x + name_size.x + 16.0, top + name_size.y / 2.0);
            paint_icon(ui, Icon::Warn, c, 16.0, p.accent);
            ui.interact(Rect::from_center_size(c, Vec2::splat(22.0)), id.with("warn"), Sense::hover())
                .on_hover_text(issue.as_str());
        }
        let tag_y = top + 22.0;
        let kind = if v.is_list { format!("{} entries · list", v.entries.len()) } else { "text".to_owned() };
        let t = paint_tag(ui, &p, pos2(x, tag_y), &kind, TagKind::Neutral);
        if v.secret {
            paint_tag(ui, &p, pos2(t.right() + 6.0, tag_y), "secret", TagKind::Accent);
        }

        // Value, with the expansion underneath.
        let key = (scope, name.clone());
        let masked = v.secret && !self.reveal.contains(&key);
        let vx = x + NAME_COL;
        let vw = rect.right() - 16.0 - ACTIONS_COL - 12.0 - vx;
        let raw = if masked { MASK } else { v.var.value.as_str() };
        let galley = elided(ui, raw, theme::mono(12.5), p.text, vw);
        let line_h = galley.size().y;
        ui.painter().galley(pos2(vx, top), galley, p.text);
        if !masked && v.exp != v.var.value {
            let galley = elided(ui, &v.exp, theme::mono(12.5), p.a700, vw);
            ui.painter().galley(pos2(vx, top + line_h + 3.0), galley, p.a700);
        }

        // Actions.
        let mut bx = rect.right() - 16.0 - 32.0;
        let btn = |bx: f32| Rect::from_min_size(pos2(bx, rect.top() + 8.0), Vec2::splat(32.0));
        if icon_btn_at(ui, btn(bx), id.with("del"), Icon::Trash, p.text).on_hover_text("Delete").clicked() {
            self.delete_var(scope, name);
        }
        bx -= 34.0;
        let edit = icon_btn_at(ui, btn(bx), id.with("edit"), Icon::Edit, p.text).on_hover_text("Edit").clicked();
        bx -= 34.0;
        if v.secret {
            let (icon, tip) = if masked { (Icon::Eye, "Reveal") } else { (Icon::EyeOff, "Hide") };
            if icon_btn_at(ui, btn(bx), id.with("eye"), icon, p.text).on_hover_text(tip).clicked() {
                if masked {
                    self.reveal.insert(key);
                } else {
                    self.reveal.remove(&key);
                }
            }
        }

        if edit || resp.double_clicked() {
            self.open_edit(scope, Some(v));
        } else if resp.clicked() && v.is_list {
            self.select(scope, Some(name.clone()));
        }
        if v.is_list {
            resp.on_hover_cursor(CursorIcon::PointingHand);
        }
    }

    // ───────────────────────────── conflicts ─────────────────────────────

    fn conflicts_view(&mut self, ui: &mut Ui, d: &Derived) {
        let p = self.pal;
        let q = self.query.trim().to_lowercase();
        let rows: Vec<&Conflict> =
            d.conflicts.iter().filter(|c| q.is_empty() || c.name.to_lowercase().contains(&q)).collect();

        ScrollArea::vertical().id_salt("conflicts").auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            Frame::new().inner_margin(Margin { left: 16, right: 16, top: 20, bottom: 20 }).show(ui, |ui| {
                label(ui, "Same name in both scopes", theme::heavy(22.0), p.text);
                ui.add_space(6.0);
                ui.scope(|ui| {
                    ui.set_max_width(640.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "Windows reads machine variables first, then user variables. A user value replaces \
                                 the machine value, except for PATH, which is joined with machine entries first.",
                            )
                            .font(theme::body(13.0))
                            .color(p.muted(0.75)),
                        )
                        .wrap(),
                    );
                });
            });

            let w = ui.available_width();
            let gap = 16.0;
            let name_w = 150.0;
            let col_w = ((w - 32.0 - name_w - gap * 3.0) / 3.0).max(60.0);
            let col_x = |left: f32, i: usize| left + 16.0 + name_w + gap + (col_w + gap) * i as f32;

            let (head, _) = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover());
            ui.painter().hline(head.x_range(), head.top() + 1.0, Stroke::new(2.0, p.divider));
            ui.painter().hline(head.x_range(), head.bottom() - 1.0, Stroke::new(2.0, p.divider));
            for (x, text) in [
                (head.left() + 16.0, "Name"),
                (col_x(head.left(), 0), "System"),
                (col_x(head.left(), 1), "User"),
                (col_x(head.left(), 2), "Effective"),
            ] {
                let g = ui.painter().layout_job(caps(text, 11.0, p.text));
                ui.painter().galley(pos2(x, head.center().y - g.size().y / 2.0), g, p.text);
            }

            if rows.is_empty() {
                ui.add_space(32.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    let msg = if d.conflicts.is_empty() {
                        "No variable is defined in both scopes."
                    } else {
                        "No conflicts match the search."
                    };
                    label(ui, msg, theme::body(14.0), p.muted(0.7));
                });
            }

            for c in rows {
                let masked = c.secret
                    && !self.reveal.contains(&(Scope::User, c.name.clone()))
                    && !self.reveal.contains(&(Scope::System, c.name.clone()));
                let show = |s: &str| if masked { MASK.to_owned() } else { s.to_owned() };
                let font = theme::mono(12.5);
                let cell = |text: String, fmt: TextFormat| {
                    let mut job = LayoutJob::single_section(text, fmt);
                    job.wrap = TextWrapping {
                        max_width: col_w,
                        max_rows: 6,
                        break_anywhere: true,
                        overflow_character: Some('…'),
                    };
                    ui.painter().layout_job(job)
                };
                let sys_fmt = if c.merged {
                    TextFormat::simple(font.clone(), p.text)
                } else {
                    let faded = p.muted(0.5);
                    TextFormat { strikethrough: Stroke::new(1.0, faded), ..TextFormat::simple(font.clone(), faded) }
                };
                let name = elided(ui, &c.name, theme::mono_bold(13.5), p.text, name_w);
                let sys = cell(show(&c.sys), sys_fmt);
                let usr = cell(show(&c.usr), TextFormat::simple(font.clone(), p.text));
                let eff_text = if c.merged { c.eff.clone() } else { show(&c.eff) };
                let eff = cell(eff_text, TextFormat::simple(theme::mono_bold(12.5), p.a700));

                let content_h =
                    (name.size().y + 8.0 + 20.0).max(sys.size().y).max(usr.size().y).max(eff.size().y);
                let (rect, _) = ui.allocate_exact_size(vec2(w, content_h + 28.0), Sense::hover());
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let top = rect.top() + 14.0;
                let name_h = name.size().y;
                ui.painter().galley(pos2(rect.left() + 16.0, top), name, p.text);
                let rule = if c.merged { "merged" } else { "user wins" };
                paint_tag(ui, &p, pos2(rect.left() + 16.0, top + name_h + 8.0), rule, TagKind::Neutral);
                ui.painter().galley(pos2(col_x(rect.left(), 0), top), sys, p.text);
                ui.painter().galley(pos2(col_x(rect.left(), 1), top), usr, p.text);
                ui.painter().galley(pos2(col_x(rect.left(), 2), top), eff, p.a700);
                ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.n300));
            }

            Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if Btn::new("Open user variables", BtnStyle::secondary(&p)).show(ui).clicked() {
                        self.set_tab(Tab::User);
                    }
                    if Btn::new("Open system variables", BtnStyle::secondary(&p)).show(ui).clicked() {
                        self.set_tab(Tab::System);
                    }
                });
            });
        });
    }

    // ───────────────────────────── list editor ─────────────────────────────

    fn list_pane(&mut self, ui: &mut Ui, scope: Scope, v: &VarInfo) {
        let p = self.pal;
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, p.bg);
        ui.painter().vline(full.left() + 1.0, full.y_range(), Stroke::new(2.0, p.divider));
        let mut r = full.with_min_x(full.left() + 2.0);
        let name = v.var.name.clone();

        // Header.
        let head = cut_top(&mut r, 100.0);
        ui.painter().hline(head.x_range(), head.bottom() - 1.0, Stroke::new(2.0, p.divider));
        let x = head.left() + 16.0;
        let kicker = ui.painter().layout_job(caps(&format!("List editor · {}", scope.label()), 11.0, p.muted(0.7)));
        ui.painter().galley(pos2(x, head.top() + 16.0), kicker, p.text);
        let title = elided(ui, &name, theme::mono_bold(24.0), p.text, head.width() - 32.0 - 150.0);
        ui.painter().galley(pos2(x, head.top() + 34.0), title, p.text);
        let over = v.len > MAX_VALUE_LEN;
        let count = ui.painter().layout_job(spans(&[
            (&format!("{} entries · separated by ;  ·  ", v.entries.len()), theme::body(12.0), p.muted(0.7)),
            (
                &format!("{} of {} characters", group_digits(v.len), group_digits(MAX_VALUE_LEN)),
                if over { theme::semi(12.0) } else { theme::body(12.0) },
                if over { p.a700 } else { p.muted(0.7) },
            ),
        ]));
        ui.painter().galley(pos2(x, head.top() + 68.0), count, p.text);
        let buttons = Rect::from_min_max(pos2(head.right() - 170.0, head.top() + 16.0), pos2(head.right() - 14.0, head.top() + 48.0));
        region(ui, buttons, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if icon_btn(ui, Icon::Close, 32.0, p.text).on_hover_text("Close").clicked() {
                    self.select(scope, None);
                }
                if Btn::new("Edit as text", BtnStyle::secondary(&p)).small().show(ui).clicked() {
                    self.open_edit(scope, Some(v));
                }
            });
        });

        // Select-all strip.
        let strip = cut_top(&mut r, 36.0);
        ui.painter().hline(strip.x_range(), strip.bottom() - 0.5, Stroke::new(1.0, p.divider));
        let n_checked = self.checked.iter().filter(|i| **i < v.entries.len()).count();
        region(ui, strip.shrink2(vec2(16.0, 0.0)), |ui| {
            ui.horizontal_centered(|ui| {
                let mut all = !v.entries.is_empty() && n_checked == v.entries.len();
                let text = spans(&[("Select all", theme::body(12.0), p.text)]);
                if check_label(ui, &p, &mut all, text) {
                    self.checked.clear();
                    if all {
                        self.checked.extend(0..v.entries.len());
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    label(ui, "Expanded paths shown in red", theme::body(12.0), p.a700);
                });
            });
        });

        let bar = (n_checked > 0).then(|| cut_bottom(&mut r, 58.0));

        region(ui, r, |ui| {
            ScrollArea::vertical().id_salt(("entries", scope, name.as_str())).auto_shrink(false).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                self.entry_rows(ui, scope, v);
                self.add_entry_row(ui, scope, &name);
            });
        });

        if let Some(rect) = bar {
            ui.painter().rect_filled(rect, 0.0, p.text);
            region(ui, rect.shrink2(vec2(16.0, 0.0)), |ui| {
                ui.horizontal_centered(|ui| {
                    label(ui, format!("{n_checked} selected"), theme::semi(13.0), p.bg);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let ghost = BtnStyle::outline(p.bg, p.n600);
                        if Btn::new("Clear", ghost).small().show(ui).clicked() {
                            self.checked.clear();
                        }
                        if Btn::new("Delete", ghost).small().icon(Icon::Trash).show(ui).clicked() {
                            self.remove_checked(scope, &name);
                        }
                        if Btn::new("Move to new variable", BtnStyle::primary(&p))
                            .small()
                            .icon(Icon::MoveOut)
                            .show(ui)
                            .clicked()
                        {
                            self.open_move(scope, &name);
                        }
                    });
                });
            });
        }
    }

    fn entry_rows(&mut self, ui: &mut Ui, scope: Scope, v: &VarInfo) {
        let p = self.pal;
        let name = v.var.name.as_str();
        let w = ui.available_width();
        let text_x = 16.0 + 16.0 + 8.0 + 16.0 + 8.0;
        let buttons_w = 26.0 * 3.0;
        let text_w = w - text_x - buttons_w - 16.0 - 6.0;
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let base = ui.scope_id().with(("entry", scope, name));
        let count = v.entries.len();
        let mut drop_on: Option<usize> = None;
        let mut last_bottom = ui.cursor().top();

        for (i, e) in v.entries.iter().enumerate() {
            let font = theme::mono(12.0);
            let raw = wrapped(ui, &e.raw, font.clone(), p.text, text_w, 4);
            let exp = (e.exp != e.raw).then(|| wrapped(ui, &e.exp, font, p.a700, text_w, 4));
            let text_h = raw.size().y + exp.as_ref().map_or(0.0, |g| g.size().y + 1.0);
            let h = (text_h + if e.issue.is_some() { 19.0 } else { 0.0 } + 20.0).max(42.0);
            let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
            last_bottom = rect.bottom();

            // Where would a dragged entry land?
            if let (Some(from), Some(pos)) = (self.drag, pointer)
                && pos.y >= rect.top()
                && pos.y < rect.bottom()
            {
                drop_on = Some(i);
                if from != i {
                    let y = if from > i { rect.top() + 1.0 } else { rect.bottom() - 2.0 };
                    ui.painter().hline(rect.x_range(), y, Stroke::new(3.0, p.accent));
                }
            }
            if !ui.is_rect_visible(rect) {
                continue;
            }

            let checked = self.checked.contains(&i);
            let dragging = self.drag == Some(i);
            if dragging {
                ui.painter().rect_filled(rect, 0.0, p.surface);
            } else if checked {
                ui.painter().rect_filled(rect, 0.0, p.a100);
            }
            ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.n300));
            let fade = if dragging { 0.4 } else { 1.0 };

            let top = rect.top() + 10.0;
            let check = Rect::from_min_size(pos2(rect.left() + 16.0, top + 1.0), Vec2::splat(16.0));
            if checkbox_at(ui, &p, check, base.with(("check", i)), checked) {
                if checked {
                    self.checked.remove(&i);
                } else {
                    self.checked.insert(i);
                }
            }

            let grip = Rect::from_min_size(pos2(rect.left() + 40.0, top + 1.0), Vec2::splat(16.0));
            let grip_resp = ui
                .interact(grip.expand(5.0), base.with(("grip", i)), Sense::drag())
                .on_hover_cursor(CursorIcon::Grab)
                .on_hover_text("Drag to reorder");
            paint_icon(ui, Icon::Grip, grip.center(), 16.0, p.muted(0.6));
            if grip_resp.drag_started() {
                self.drag = Some(i);
            }

            let tx = rect.left() + text_x;
            let mut y = top;
            let raw_h = raw.size().y;
            ui.painter().galley(pos2(tx, y), raw, p.text.gamma_multiply(fade));
            y += raw_h + 1.0;
            if let Some(g) = exp {
                let gh = g.size().y;
                ui.painter().galley(pos2(tx, y), g, p.a700);
                y += gh + 1.0;
            }
            if let Some(issue) = e.issue {
                paint_icon(ui, Icon::Warn, pos2(tx + 6.0, y + 9.0), 12.0, p.a700);
                ui.painter().text(pos2(tx + 17.0, y + 9.0), Align2::LEFT_CENTER, issue.label(), theme::semi(11.0), p.a700);
            }

            let mut bx = rect.right() - 16.0 - buttons_w;
            let mut slot = || {
                let r = Rect::from_min_size(pos2(bx, rect.top() + 6.0), Vec2::splat(26.0));
                bx += 26.0;
                r
            };
            let (up, down, remove) = (slot(), slot(), slot());
            if i > 0 {
                if icon_btn_at(ui, up, base.with(("up", i)), Icon::Up, p.text).on_hover_text("Move up").clicked() {
                    self.move_entry(scope, name, i, i - 1);
                }
            } else {
                paint_icon(ui, Icon::Up, up.center(), 14.0, p.muted(0.25));
            }
            if i + 1 < count {
                if icon_btn_at(ui, down, base.with(("down", i)), Icon::Down, p.text).on_hover_text("Move down").clicked() {
                    self.move_entry(scope, name, i, i + 1);
                }
            } else {
                paint_icon(ui, Icon::Down, down.center(), 14.0, p.muted(0.25));
            }
            if icon_btn_at(ui, remove, base.with(("remove", i)), Icon::Close, p.text).on_hover_text("Remove").clicked() {
                self.remove_entry(scope, name, i);
            }
        }

        // Finish (or continue) a drag.
        if let Some(from) = self.drag {
            if ui.input(|i| i.pointer.any_down()) {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            } else {
                let to = drop_on.or_else(|| pointer.filter(|pos| pos.y >= last_bottom).map(|_| count.saturating_sub(1)));
                if let Some(to) = to {
                    self.move_entry(scope, name, from, to);
                }
                self.drag = None;
            }
        }
    }

    fn add_entry_row(&mut self, ui: &mut Ui, scope: Scope, name: &str) {
        let p = self.pal;
        Frame::new().inner_margin(Margin::symmetric(16, 12)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let w = ui.available_width() - 8.0 - 58.0;
                let field = text_input(ui, &p, &mut self.new_entry, r"Add entry, e.g. %USERPROFILE%\bin", w, theme::mono(12.0));
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let add = Btn::new("Add", BtnStyle::secondary(&p)).show(ui).clicked();
                if enter || add {
                    let new: Vec<String> = crate::model::split_list(&self.new_entry);
                    if !new.is_empty() {
                        self.edit_list(scope, name, |items| items.extend(new));
                        self.new_entry.clear();
                        if enter {
                            field.request_focus();
                        }
                    }
                }
            });
        });
    }

    // ───────────────────────────── bars ─────────────────────────────

    fn dirty_bar(&mut self, ui: &mut Ui, d: &Derived) {
        let p = self.pal;
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, p.accent);
        let n = changed_var_count(&d.changes);
        let needs_admin = !self.elevated && d.changes.iter().any(|c| c.scope == Scope::System);
        let mut text = format!("{n} unsaved change{}", if n == 1 { "" } else { "s" });
        if needs_admin {
            text.push_str(" · includes system scope, administrator approval required");
        }
        region(ui, rect.shrink2(vec2(16.0, 0.0)), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter().rect_filled(dot, 0.0, p.bg);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let busy = self.applying();
                    let apply = if busy { "Applying…" } else { "Apply changes" };
                    if Btn::new(apply, BtnStyle::solid(p.bg, p.text)).enabled(!busy).show(ui).clicked() {
                        self.start_apply(ui.ctx());
                    }
                    if Btn::new("Discard", BtnStyle::outline(p.bg, p.bg)).enabled(!busy).show(ui).clicked() {
                        self.discard();
                    }
                    ui.add_space(8.0);
                    // Whatever room is left goes to the message; hovering it lists what will be written.
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let summary: Vec<String> = d
                            .changes
                            .iter()
                            .map(|c| {
                                let verb = if c.op == crate::model::ChangeOp::Delete { "delete" } else { "set" };
                                format!("{} · {verb} {}", c.scope.label(), c.name)
                            })
                            .collect();
                        ui.add(egui::Label::new(RichText::new(text).font(theme::semi(14.0)).color(p.bg)).truncate())
                            .on_hover_text(summary.join("\n"));
                    });

                });
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui, d: &Derived) {
        let p = self.pal;
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, p.surface);
        ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, p.divider));
        region(ui, rect.shrink2(vec2(16.0, 0.0)), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if let Some((done, total)) = self.validator.progress() {
                    label(ui, "Validating paths in background", theme::body(12.0), p.text);
                    let (bar, _) = ui.allocate_exact_size(vec2(160.0, 6.0), Sense::hover());
                    ui.painter().rect_filled(bar, 0.0, p.n300);
                    let frac = if total == 0 { 1.0 } else { done as f32 / total as f32 };
                    ui.painter().rect_filled(bar.with_max_x(bar.left() + bar.width() * frac), 0.0, p.accent);
                    label(ui, format!("{done} / {total}"), theme::mono(12.0), p.text);
                } else {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    icon(ui, Icon::Check, 14.0, p.text);
                    let result = match d.bad_paths {
                        0 => ("all paths found".to_owned(), theme::body(12.0), p.text),
                        1 => ("1 path not found".to_owned(), theme::semi(12.0), p.a700),
                        n => (format!("{n} paths not found"), theme::semi(12.0), p.a700),
                    };
                    ui.label(spans(&[
                        ("Validation finished · ", theme::body(12.0), p.text),
                        (&result.0, result.1, result.2),
                    ]));
                    ui.add_space(6.0);
                    let again = ui
                        .add(
                            egui::Label::new(
                                RichText::new("Check again").font(theme::body(12.0)).color(p.muted(0.7)).underline(),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_cursor(CursorIcon::PointingHand);
                    if again.clicked() {
                        self.recheck_paths();
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let backup = match &self.last_backup {
                        Some(t) => format!("Last backup: {}", store::friendly_time(t).to_lowercase()),
                        None => "No backups yet".to_owned(),
                    };
                    let text = format!("{backup} · {} · {} variables", self.os, self.work.total());
                    label(ui, text, theme::body(12.0), theme::mix(p.surface, p.text, 0.75));
                });
            });
        });
    }

    fn toast_ui(&mut self, ctx: &egui::Context, lift: f32) {
        let p = self.pal;
        let Some(t) = &self.toast else { return };
        let offset = vec2(-16.0, -(48.0 + lift));
        let clicked = Area::new(Id::unique("envoy-toast"))
            .order(Order::Middle)
            .anchor(Align2::RIGHT_BOTTOM, offset)
            .show(ctx, |ui| {
                let frame = Frame::new()
                    .fill(p.text)
                    .inner_margin(Margin::symmetric(16, 14))
                    .shadow(ctx.global_style().visuals.window_shadow);
                let inner = frame.show(ui, |ui| {
                    ui.set_width(368.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing = vec2(12.0, 3.0);
                        icon(ui, if t.error { Icon::Warn } else { Icon::Check }, 18.0, p.a400);
                        ui.vertical(|ui| {
                            ui.add(egui::Label::new(RichText::new(&t.title).font(theme::semi(13.0)).color(p.bg)).wrap());
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&t.body).font(theme::body(13.0)).color(theme::mix(p.text, p.bg, 0.8)),
                                )
                                .wrap(),
                            );
                        });
                    });
                });
                ui.interact(inner.response.rect, Id::unique("envoy-toast-click"), Sense::click()).clicked()
            })
            .inner;
        if clicked {
            self.toast = None;
        }
    }
}

/// A full-width notice above the table; takes its height off the top of `r`.
fn banner(ui: &mut Ui, p: &Palette, r: &mut Rect, icon: Icon, lead: &str, rest: &str) {
    let mut job = spans(&[(lead, theme::semi(13.0), p.a900), (rest, theme::body(13.0), p.a900)]);
    job.wrap.max_width = (r.width() - 32.0 - 30.0).max(100.0);
    let galley = ui.painter().layout_job(job);
    let rect = cut_top(r, galley.size().y + 22.0);
    ui.painter().rect_filled(rect, 0.0, p.a100);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.divider));
    paint_icon(ui, icon, pos2(rect.left() + 16.0 + 9.0, rect.top() + 11.0 + 9.0), 18.0, p.a900);
    ui.painter().galley(pos2(rect.left() + 16.0 + 30.0, rect.top() + 11.0), galley, p.a900);
}


