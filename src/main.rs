mod style;
mod sync;

use eframe::egui::{self, Align, Layout, Response, RichText, Stroke, TextStyle, Ui};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};
use style::Palette;
use sync::{Ctx, Data, Overview, Prefer, SetupState, SyncError};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Omarchy Sync")
            .with_app_id("omarchy-sync")
            .with_inner_size([780.0, 660.0])
            .with_min_inner_size([520.0, 460.0]),
        ..Default::default()
    };
    eframe::run_native("Omarchy Sync", options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}

enum Outcome {
    Probed(SetupState),
    Refreshed(Result<Overview, String>),
    Synced(Result<Vec<String>, SyncError>),
    /// Holds the backup path when this computer joined existing settings.
    SetUp(Result<Option<PathBuf>, String>),
    Done(Result<(), String>),
}

enum Msg {
    Step(String),
    Finished(Outcome),
}

struct Job {
    rx: Receiver<Msg>,
    step: String,
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Sync,
    Apps,
    Folders,
}

struct Notice {
    good: bool,
    text: String,
    details: Option<String>,
}

struct App {
    sx: Ctx,
    p: Palette,
    setup: Option<SetupState>,
    tab: Tab,
    job: Option<Job>,
    data: Data,
    ov: Option<Overview>,
    notice: Option<Notice>,
    conflict: Option<Vec<String>>,
    signing_in: bool,
    /// Edited apps or folders that haven't gone out yet.
    pending: bool,
    refreshed: Instant,
}

impl App {
    fn new(ctx: &egui::Context) -> App {
        let p = Palette::load();
        style::apply(ctx, &p);
        let mut app = App {
            sx: Ctx::load(),
            p,
            setup: None,
            tab: Tab::Sync,
            job: None,
            data: Data::default(),
            ov: None,
            notice: None,
            conflict: None,
            signing_in: false,
            pending: false,
            refreshed: Instant::now(),
        };
        app.probe(ctx);
        app
    }

    fn ready(&self) -> bool {
        self.setup.as_ref().is_some_and(|s| s.ready)
    }

    fn spawn(&mut self, ctx: &egui::Context, first_step: &str, work: impl FnOnce(&Ctx, &dyn Fn(&str)) -> Outcome + Send + 'static) {
        let (tx, rx) = channel();
        let (sx, ctx) = (self.sx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let (t, c) = (tx.clone(), ctx.clone());
            let step = move |s: &str| {
                let _ = t.send(Msg::Step(s.to_string()));
                c.request_repaint();
            };
            let out = work(&sx, &step);
            let _ = tx.send(Msg::Finished(out));
            ctx.request_repaint();
        });
        self.job = Some(Job { rx, step: first_step.to_string() });
    }

    fn probe(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, "Getting ready", |sx, _| Outcome::Probed(sync::probe(sx)));
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.refreshed = Instant::now();
        self.spawn(ctx, "Checking for changes", |sx, _| Outcome::Refreshed(sync::overview(sx)));
    }

    fn start_sync(&mut self, ctx: &egui::Context, prefer: Prefer) {
        self.notice = None;
        self.spawn(ctx, "Starting", move |sx, step| Outcome::Synced(sync::sync(sx, prefer, step)));
    }

    fn save(&mut self) {
        match self.sx.save_data(&self.data) {
            Ok(()) => self.pending = true,
            Err(e) => self.fail(&e),
        }
    }

    fn fail(&mut self, err: &str) {
        self.notice = Some(Notice { good: false, text: sync::friendly(err), details: Some(err.to_string()) });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.job else {
            // Quietly look for news from the other computers every few minutes.
            if self.ready() && self.refreshed.elapsed() > Duration::from_secs(300) {
                self.refresh(ctx);
            }
            ctx.request_repaint_after(Duration::from_secs(60));
            return;
        };
        let mut done = None;
        while let Ok(msg) = job.rx.try_recv() {
            match msg {
                Msg::Step(s) => job.step = s,
                Msg::Finished(o) => done = Some(o),
            }
        }
        if let Some(outcome) = done {
            self.job = None;
            self.finish(ctx, outcome);
        }
    }

    fn finish(&mut self, ctx: &egui::Context, outcome: Outcome) {
        match outcome {
            Outcome::Probed(s) => {
                let ready = s.ready;
                self.setup = Some(s);
                if ready {
                    self.data = self.sx.load_data();
                    self.refresh(ctx);
                }
            }
            Outcome::Refreshed(Ok(ov)) => {
                self.ov = Some(ov);
                self.data = self.sx.load_data();
            }
            Outcome::Refreshed(Err(e)) => self.fail(&e),
            Outcome::Synced(Ok(notes)) => {
                let mut text = "Synced.".to_string();
                for n in notes {
                    text = format!("{text} {n}");
                }
                self.notice = Some(Notice { good: true, text, details: None });
                self.conflict = None;
                self.pending = false;
                self.refresh(ctx);
            }
            Outcome::Synced(Err(SyncError::Conflict(files))) => self.conflict = Some(files),
            Outcome::Synced(Err(SyncError::Failed(e))) => {
                self.fail(&e);
                self.refresh(ctx);
            }
            Outcome::SetUp(Ok(backup)) => {
                if let Some(s) = &mut self.setup {
                    s.ready = true;
                }
                self.data = self.sx.load_data();
                self.start_sync(ctx, Prefer::Ask);
                if let Some(b) = backup {
                    let text = format!("Your previous settings were backed up to {}.", b.display());
                    self.notice = Some(Notice { good: true, text, details: None });
                }
            }
            Outcome::SetUp(Err(e)) => self.fail(&e),
            Outcome::Done(r) => {
                if let Err(e) = r {
                    self.fail(&e);
                }
                if self.ready() { self.refresh(ctx) } else { self.probe(ctx) }
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let frame = egui::Frame::new().fill(self.p.bg).inner_margin(egui::Margin::symmetric(40, 28));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                // One readable column, centred when the window is wide.
                let width = ui.available_width().min(660.0);
                ui.horizontal_top(|ui| {
                    ui.add_space((ui.available_width() - width) / 2.0);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        self.screen(ui);
                    });
                });
            });
        });
    }
}

impl App {
    fn screen(&mut self, ui: &mut Ui) {
        match self.setup.clone() {
            None => busy_line(ui, "Getting ready"),
            Some(s) if !s.ready => self.setup_screen(ui, &s),
            Some(_) => self.main_screen(ui),
        }
    }
}

// ---------- Screens ----------

impl App {
    fn setup_screen(&mut self, ui: &mut Ui, s: &SetupState) {
        let p = self.p;
        let ctx = ui.ctx().clone();
        ui.add_space(24.0);
        ui.label(RichText::new("Keep your computers in step").heading());
        ui.add_space(6.0);
        ui.label(
            RichText::new("Your Omarchy settings, themes, plugins and apps, the same on every computer. They're kept privately in your GitHub account.")
                .color(p.quiet),
        );
        ui.add_space(28.0);

        let last = if s.found { "Bring in your synced settings" } else { "Save this computer's settings" };
        let current = if !s.missing.is_empty() { 0 } else if !s.signed_in { 1 } else { 2 };
        for (i, text) in ["Install the sync helpers", "Sign in to GitHub", last].into_iter().enumerate() {
            step_row(ui, &p, i, current, text);
        }
        ui.add_space(24.0);

        if let Some(job) = &self.job {
            busy_line(ui, &job.step);
        } else if current == 0 {
            hint(ui, &p, &format!("Installs {}. Omarchy will ask for your password.", s.missing.join(" and ")));
            if primary(ui, &p, "Install helpers").clicked() {
                let pkgs = s.missing.clone();
                self.spawn(&ctx, "Installing helpers", move |_, _| Outcome::Done(sync::install_helpers(&pkgs)));
            }
        } else if current == 1 && !self.signing_in {
            hint(ui, &p, "A window opens with a short code to confirm in your browser.");
            if primary(ui, &p, "Sign in to GitHub").clicked() {
                match sync::sign_in() {
                    Ok(()) => self.signing_in = true,
                    Err(e) => self.fail(&e),
                }
            }
        } else if current == 1 {
            hint(ui, &p, "Finish signing in in the window that opened, then come back here.");
            ui.horizontal(|ui| {
                if primary(ui, &p, "I've signed in").clicked() {
                    self.probe(&ctx);
                }
                if ui.link("Open the sign-in window again").clicked() {
                    let _ = sync::sign_in();
                }
            });
        } else if s.found {
            hint(ui, &p, "This computer's settings are replaced by your synced ones. A backup is saved first.");
            if primary(ui, &p, "Use my synced settings").clicked() {
                let login = s.login.clone().unwrap_or_default();
                self.spawn(&ctx, "Getting ready", move |sx, step| Outcome::SetUp(sync::join(sx, &login, step).map(Some)));
            }
        } else {
            hint(ui, &p, "This is your first computer. Its settings become the starting point for the others.");
            if primary(ui, &p, "Start syncing").clicked() {
                self.spawn(&ctx, "Getting ready", |sx, step| Outcome::SetUp(sync::start(sx, step).map(|_| None)));
            }
        }
        self.notice_ui(ui);
    }

    fn main_screen(&mut self, ui: &mut Ui) {
        let p = self.p;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 22.0;
            for (tab, name) in [(Tab::Sync, "Sync"), (Tab::Apps, "Apps"), (Tab::Folders, "What's synced")] {
                let on = self.tab == tab;
                let text = RichText::new(name).text_style(title()).color(if on { p.ink } else { p.quiet });
                let r = ui.add(egui::Button::new(text).frame(false));
                if on {
                    ui.painter().hline(r.rect.x_range(), r.rect.bottom() + 3.0, Stroke::new(2.0, p.accent));
                }
                focus_ring(ui, &p, &r);
                if r.clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.add_space(30.0);

        if self.pending && self.job.is_none() && self.tab != Tab::Sync {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Your other computers get this change the next time you sync.").color(p.warn));
                if ui.button("Sync now").clicked() {
                    self.tab = Tab::Sync;
                    self.start_sync(&ui.ctx().clone(), Prefer::Ask);
                }
            });
            ui.add_space(16.0);
        }

        match self.tab {
            Tab::Sync => self.sync_tab(ui),
            Tab::Apps => {
                let busy = self.job.is_some();
                ui.add_enabled_ui(!busy, |ui| self.apps_tab(ui));
            }
            Tab::Folders => {
                let busy = self.job.is_some();
                ui.add_enabled_ui(!busy, |ui| self.folders_tab(ui));
            }
        }
    }

    fn headline(&self) -> (String, String) {
        if let Some(job) = &self.job {
            return (format!("{}…", job.step), "You can keep working while this runs.".into());
        }
        if self.conflict.is_some() {
            return (
                "The same setting was changed on two computers.".into(),
                "Choose which version to keep. Changes to other settings are kept either way.".into(),
            );
        }
        let Some(ov) = &self.ov else { return ("Checking for changes…".into(), String::new()) };
        let changed = sync::plural(ov.unshared.len(), "setting");
        let updates = sync::plural(ov.incoming, "update");
        let head = match (ov.unshared.len(), ov.incoming) {
            (0, 0) => "Everything matches your other computers.".to_string(),
            (_, 0) => format!("You changed {changed} on this computer."),
            (0, _) => format!("Your other computers have {updates} for this one."),
            _ => format!("You changed {changed} here, and your other computers have {updates}."),
        };
        let detail = if !ov.online {
            "You're offline. Syncing works again once you're connected.".to_string()
        } else {
            match ov.computers.iter().find(|(n, _)| *n == self.sx.host).and_then(|(_, t)| *t) {
                Some(t) => format!("This computer last synced {}.", sync::ago(sync::now() - t)),
                None => "This computer hasn't synced yet.".to_string(),
            }
        };
        (head, detail)
    }

    fn sync_tab(&mut self, ui: &mut Ui) {
        let p = self.p;
        let ctx = ui.ctx().clone();
        let (head, detail) = self.headline();
        if self.job.is_some() {
            ui.add(egui::Spinner::new().size(26.0).color(p.accent));
        }
        ui.add(egui::Label::new(RichText::new(head).heading()).wrap());
        if !detail.is_empty() {
            ui.label(RichText::new(detail).color(p.quiet));
        }
        ui.add_space(20.0);

        if let Some(files) = self.conflict.clone() {
            for name in labels(&files).keys() {
                ui.label(format!("• {name}"));
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if big(ui, &p, "Keep this computer's version", false).clicked() {
                    self.start_sync(&ctx, Prefer::ThisComputer);
                }
                if big(ui, &p, "Keep the other computer's version", false).clicked() {
                    self.start_sync(&ctx, Prefer::OtherComputer);
                }
            });
        } else if ui.add_enabled_ui(self.job.is_none(), |ui| primary(ui, &p, "Sync now")).inner.clicked() {
            self.start_sync(&ctx, Prefer::Ask);
        }
        self.notice_ui(ui);

        let Some(ov) = self.ov.clone() else { return };
        if !ov.unshared.is_empty() {
            section(ui, &p, "Not shared yet");
            for (name, files) in labels(&ov.unshared) {
                ui.label(name);
                let mut list = files.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
                if files.len() > 5 {
                    list = format!("{list} and {} more", files.len() - 5);
                }
                if !list.is_empty() {
                    ui.label(RichText::new(list).small().color(p.quiet));
                }
                ui.add_space(4.0);
            }
        }

        section(ui, &p, "Your computers");
        for (name, when) in &ov.computers {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).text_style(if *name == self.sx.host { title() } else { TextStyle::Body }));
                if *name == self.sx.host {
                    ui.label(RichText::new("this computer").color(p.quiet));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let text = when.map_or("hasn't synced yet".to_string(), |t| format!("synced {}", sync::ago(sync::now() - t)));
                    ui.label(RichText::new(text).color(p.quiet));
                });
            });
        }
    }

    fn apps_tab(&mut self, ui: &mut Ui) {
        let p = self.p;
        let host = self.sx.host.clone();
        let apps = self.ov.as_ref().map(|o| o.apps.clone()).unwrap_or_default();
        let mut computers: Vec<String> = self.data.computers.keys().filter(|c| **c != host).cloned().collect();
        computers.insert(0, host.clone());
        let mut changed = false;

        ui.label(RichText::new("Synced apps").text_style(title()));
        ui.label(RichText::new("A ticked app is installed on that computer when it syncs. Unticking never uninstalls anything.").color(p.quiet));
        ui.add_space(12.0);
        if self.data.apps.is_empty() {
            ui.label(RichText::new("No apps are synced yet. Pick some from the list below.").color(p.quiet));
        } else {
            // Fixed columns: app name and description on the left, one switch per computer, then the remove link.
            let cell = 104.0;
            let name_w = (ui.available_width() - cell * computers.len() as f32 - 110.0).max(180.0);
            let cells = |ui: &mut Ui, w: f32, add: &mut dyn FnMut(&mut Ui)| {
                ui.allocate_ui_with_layout(egui::vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(w);
                    add(ui);
                });
            };
            ui.horizontal(|ui| {
                cells(ui, name_w, &mut |ui| {
                    ui.label(RichText::new("App").small().color(p.quiet));
                });
                for c in &computers {
                    let name = if *c == host { "This computer" } else { c.as_str() };
                    cells(ui, cell, &mut |ui| {
                        ui.add(egui::Label::new(RichText::new(name).small().color(p.quiet)).truncate());
                    });
                }
            });
            ui.add_space(4.0);
            for app in self.data.apps.clone() {
                ui.horizontal_top(|ui| {
                    cells(ui, name_w, &mut |ui| {
                        ui.label(&app);
                        if let Some(about) = apps.about.get(&app) {
                            ui.label(RichText::new(about).small().color(p.quiet));
                        } else if self.data.wants_app(&host, &app) && !apps.installed.contains(&app) {
                            ui.label(RichText::new("Installs here on the next sync").small().color(p.warn));
                        }
                    });
                    for c in &computers {
                        cells(ui, cell, &mut |ui| {
                            let mut on = self.data.wants_app(c, &app);
                            if toggle(ui, &p, &mut on, &format!("{app} on {c}")).changed() {
                                self.data.set_app(c, &app, on);
                                changed = true;
                            }
                        });
                    }
                    let stop = ui.add(egui::Button::new(RichText::new("Stop syncing").small().color(p.quiet)).frame(false));
                    focus_ring(ui, &p, &stop);
                    if stop.clicked() {
                        sync::toggle(&mut self.data.apps, &app, false);
                        changed = true;
                    }
                });
                ui.add_space(14.0);
            }
        }

        section(ui, &p, "Installed only on this computer");
        if apps.local_only.is_empty() {
            ui.label(RichText::new("Nothing here. Every app you installed yourself is synced.").color(p.quiet));
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Apps you installed here that your other computers don't have.").color(p.quiet));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(format!("Sync all {}", apps.local_only.len())).clicked() {
                        self.data.apps.extend(apps.local_only.iter().cloned());
                        changed = true;
                    }
                });
            });
            ui.add_space(8.0);
            for app in &apps.local_only {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(app);
                        if let Some(about) = apps.about.get(app) {
                            ui.label(RichText::new(short(about)).small().color(p.quiet));
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Sync to all computers").clicked() {
                            sync::toggle(&mut self.data.apps, app, true);
                            changed = true;
                        }
                    });
                });
                ui.add_space(4.0);
            }
        }
        if changed {
            if let Some(ov) = &mut self.ov {
                ov.apps.local_only.retain(|a| !self.data.apps.contains(a));
            }
            self.save();
        }
    }

    fn folders_tab(&mut self, ui: &mut Ui) {
        let p = self.p;
        let ctx = ui.ctx().clone();
        let host = self.sx.host.clone();
        ui.label(RichText::new("Turn on what should be the same on every computer.").color(p.quiet));
        ui.add_space(14.0);

        for (path, name, _) in sync::FOLDERS {
            let synced = self.data.folders.iter().any(|f| f == path);
            if !synced && !self.sx.home.join(path).exists() {
                continue;
            }
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(*name);
                    ui.label(RichText::new(format!("~/{path}")).small().color(p.quiet));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut on = synced;
                    if toggle(ui, &p, &mut on, name).changed() {
                        sync::toggle(&mut self.data.folders, path, on);
                        self.save();
                        self.spawn(&ctx, if on { "Adding" } else { "Removing" }, move |sx, _| Outcome::Done(sync::track(sx, path, on)));
                    }
                });
            });
            if synced {
                let here = self.data.uses_folder(&host, path);
                ui.horizontal(|ui| {
                    if !here {
                        ui.label(RichText::new("This computer keeps its own version.").small().color(p.warn));
                    }
                    let text = if here { "Keep a separate version on this computer" } else { "Use the synced version" };
                    let r = ui.add(egui::Button::new(RichText::new(text).small().color(p.quiet)).frame(false));
                    focus_ring(ui, &p, &r);
                    if r.clicked() {
                        self.data.set_folder_here(&host, path, !here);
                        self.save();
                    }
                });
            }
            ui.add_space(10.0);
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new("Your display layout (monitors.lua) always stays separate on each computer, because every screen is different.")
                .small()
                .color(p.quiet),
        );
    }

    fn notice_ui(&mut self, ui: &mut Ui) {
        let p = self.p;
        let Some(n) = &self.notice else { return };
        ui.add_space(14.0);
        ui.horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 20.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, if n.good { p.good } else { p.bad });
            ui.add(egui::Label::new(&n.text).wrap());
        });
        if let Some(d) = &n.details {
            ui.collapsing(RichText::new("Details").small().color(p.quiet), |ui| {
                ui.label(RichText::new(d).monospace().color(p.quiet));
            });
        }
    }
}

// ---------- Small building blocks ----------

fn title() -> TextStyle {
    TextStyle::Name("title".into())
}

fn short(s: &str) -> String {
    if s.chars().count() > 56 { format!("{}…", s.chars().take(55).collect::<String>()) } else { s.to_string() }
}

/// Groups paths under home by their plain folder name, with file names.
fn labels(paths: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in paths {
        let entry = out.entry(sync::label(path).to_string()).or_default();
        if path != sync::DATA_LABEL {
            entry.push(path.rsplit('/').next().unwrap_or(path).to_string());
        }
    }
    out
}

fn primary(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    big(ui, p, text, true)
}

fn big(ui: &mut Ui, p: &Palette, text: &str, accent: bool) -> Response {
    let label = RichText::new(text).text_style(title()).color(if accent { p.bg } else { p.ink });
    let fill = if accent { p.accent } else { p.raised };
    let r = ui.add(egui::Button::new(label).fill(fill).min_size(egui::vec2(172.0, 46.0)).corner_radius(10));
    focus_ring(ui, p, &r);
    r
}

/// Keyboard focus must stay visible, including on accent-filled and frameless buttons.
fn focus_ring(ui: &Ui, p: &Palette, r: &Response) {
    if r.has_focus() {
        ui.painter().rect_stroke(r.rect.expand(4.0), 12.0, Stroke::new(2.0, p.ink), egui::StrokeKind::Outside);
    }
}

fn hint(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text).color(p.quiet));
    ui.add_space(10.0);
}

fn busy_line(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(format!("{text}…"));
    });
}

fn section(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(30.0);
    ui.label(RichText::new(text).text_style(title()));
    let y = ui.cursor().top() + 2.0;
    ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0, p.raised));
    ui.add_space(12.0);
}

/// One row of the setup checklist; `current` is the step being worked on.
fn step_row(ui: &mut Ui, p: &Palette, i: usize, current: usize, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
        let done = i < current;
        let (fill, ink) = if done {
            (p.good, p.bg)
        } else if i == current {
            (p.accent, p.bg)
        } else {
            (p.raised, p.quiet)
        };
        ui.painter().circle_filled(rect.center(), 13.0, fill);
        let mark = if done { "✔".to_string() } else { (i + 1).to_string() };
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, mark, egui::FontId::new(14.0, style::bold()), ink);
        let color = if i == current { p.ink } else { p.quiet };
        ui.label(RichText::new(text).text_style(if i == current { title() } else { TextStyle::Body }).color(color));
    });
    ui.add_space(6.0);
}

fn toggle(ui: &mut Ui, p: &Palette, on: &mut bool, what: &str) -> Response {
    switch(ui, p, on, what, 1.0)
}

/// An on/off switch; `what` is read out by screen readers.
fn switch(ui: &mut Ui, p: &Palette, on: &mut bool, what: &str, scale: f32) -> Response {
    let size = egui::vec2(44.0, 24.0) * scale;
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *on, what));
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let radius = rect.height() / 2.0;
    let track = if *on { p.accent } else { p.raised };
    let track = if ui.is_enabled() { track } else { track.gamma_multiply(0.5) };
    ui.painter().rect_filled(rect, radius, track);
    focus_ring(ui, p, &resp);
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), radius - 4.0 * scale, if *on { p.bg } else { p.quiet });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
