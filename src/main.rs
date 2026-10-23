mod style;
mod sync;

use iced::futures::{SinkExt, StreamExt};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::{Column, button, column, container, row, rule, scrollable, space, text, toggler};
use iced::{Alignment, Element, Font, Length, Subscription, Task, window};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use style::Palette;
use sync::{Ctx, Data, Overview, Prefer, SetupState, SyncError};

fn main() -> iced::Result {
    let (font, head, files) = style::fonts();
    let mut app = iced::application(move || App::new(head), App::update, App::view)
        .title("Omarchy Sync")
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(font)
        .window(window::Settings {
            size: iced::Size::new(780.0, 680.0),
            min_size: Some(iced::Size::new(520.0, 460.0)),
            platform_specific: window::settings::PlatformSpecific { application_id: "omarchy-sync".into(), ..Default::default() },
            ..Default::default()
        });
    for f in files {
        app = app.font(f);
    }
    app.run()
}

#[derive(Debug, Clone)]
enum Outcome {
    Probed(SetupState),
    Refreshed(Result<Overview, String>),
    Synced(Result<Vec<String>, SyncError>),
    /// Holds the backup path when this computer joined existing settings.
    SetUp(Result<Option<PathBuf>, String>),
    Done(Result<(), String>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tab {
    Sync,
    Apps,
    Folders,
}

#[derive(Debug, Clone)]
enum Message {
    Step(String),
    Finished(Outcome),
    Pulse,
    Refresh,
    Key(keyboard::Event),
    Tab(Tab),
    Sync(Prefer),
    ShowDetails,
    InstallHelpers,
    SignIn,
    SignedIn,
    Start,
    Join,
    SetApp(String, String, bool),
    StopApp(String),
    AddApps(Vec<String>),
    SetFolder(&'static str, bool),
    FolderHere(&'static str, bool),
}

struct Notice {
    good: bool,
    text: String,
    details: Option<String>,
}

struct App {
    sx: Ctx,
    p: Palette,
    /// Bold heading font; body text uses the app default.
    head: Font,
    setup: Option<SetupState>,
    tab: Tab,
    /// The step being worked on while a background job runs.
    busy: Option<String>,
    pulse: f32,
    data: Data,
    ov: Option<Overview>,
    notice: Option<Notice>,
    show_details: bool,
    conflict: Option<Vec<String>>,
    signing_in: bool,
    /// Edited apps or folders that haven't gone out yet.
    pending: bool,
}

/// Runs blocking work on its own thread, streaming progress steps back to the UI.
fn job(sx: Ctx, work: impl FnOnce(&Ctx, &dyn Fn(&str)) -> Outcome + Send + 'static) -> Task<Message> {
    Task::run(
        iced::stream::channel(16, async move |mut out: iced::futures::channel::mpsc::Sender<Message>| {
            let (tx, mut rx) = iced::futures::channel::mpsc::unbounded();
            std::thread::spawn(move || {
                let step = |s: &str| {
                    let _ = tx.unbounded_send(Message::Step(s.to_string()));
                };
                let outcome = work(&sx, &step);
                let _ = tx.unbounded_send(Message::Finished(outcome));
            });
            while let Some(m) = rx.next().await {
                let _ = out.send(m).await;
            }
        }),
        |m| m,
    )
}

impl App {
    fn new(head: Font) -> (App, Task<Message>) {
        let mut app = App {
            sx: Ctx::load(),
            p: Palette::load(),
            head,
            setup: None,
            tab: Tab::Sync,
            busy: None,
            pulse: 0.0,
            data: Data::default(),
            ov: None,
            notice: None,
            show_details: false,
            conflict: None,
            signing_in: false,
            pending: false,
        };
        let task = app.probe();
        (app, task)
    }

    fn theme(&self) -> iced::Theme {
        self.p.theme()
    }

    fn ready(&self) -> bool {
        self.setup.as_ref().is_some_and(|s| s.ready)
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![keyboard::listen().map(Message::Key)];
        if self.busy.is_some() {
            subs.push(iced::time::every(Duration::from_millis(50)).map(|_| Message::Pulse));
        } else if self.ready() {
            // Quietly look for news from the other computers every few minutes.
            subs.push(iced::time::every(Duration::from_secs(300)).map(|_| Message::Refresh));
        }
        Subscription::batch(subs)
    }

    fn run(&mut self, first_step: &str, work: impl FnOnce(&Ctx, &dyn Fn(&str)) -> Outcome + Send + 'static) -> Task<Message> {
        if self.busy.is_some() {
            return Task::none();
        }
        self.busy = Some(first_step.to_string());
        job(self.sx.clone(), work)
    }

    fn refresh(&mut self) -> Task<Message> {
        self.run("Checking for changes", |sx, _| Outcome::Refreshed(sync::overview(sx)))
    }

    fn probe(&mut self) -> Task<Message> {
        self.run("Getting ready", |sx, _| Outcome::Probed(sync::probe(sx)))
    }

    fn save(&mut self) {
        match self.sx.save_data(&self.data) {
            Ok(()) => self.pending = true,
            Err(e) => self.fail(&e),
        }
    }

    fn fail(&mut self, err: &str) {
        self.show_details = false;
        self.notice = Some(Notice { good: false, text: sync::friendly(err), details: Some(err.to_string()) });
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Step(s) => {
                if self.busy.is_some() {
                    self.busy = Some(s);
                }
                Task::none()
            }
            Message::Finished(outcome) => {
                self.busy = None;
                self.finish(outcome)
            }
            Message::Pulse => {
                self.pulse += 0.12;
                Task::none()
            }
            Message::Refresh => self.refresh(),
            Message::Key(keyboard::Event::KeyPressed { key, modifiers, .. }) => self.shortcut(key, modifiers),
            Message::Key(_) => Task::none(),
            Message::Tab(t) => {
                self.tab = t;
                Task::none()
            }
            Message::Sync(prefer) => {
                self.notice = None;
                self.tab = Tab::Sync;
                self.run("Starting", move |sx, step| Outcome::Synced(sync::sync(sx, prefer, step)))
            }
            Message::ShowDetails => {
                self.show_details = true;
                Task::none()
            }
            Message::InstallHelpers => {
                let pkgs = self.setup.as_ref().map(|s| s.missing.clone()).unwrap_or_default();
                self.run("Installing helpers", move |_, _| Outcome::Done(sync::install_helpers(&pkgs)))
            }
            Message::SignIn => {
                match sync::sign_in() {
                    Ok(()) => self.signing_in = true,
                    Err(e) => self.fail(&e),
                }
                Task::none()
            }
            Message::SignedIn => self.probe(),
            Message::Start => self.run("Getting ready", |sx, step| Outcome::SetUp(sync::start(sx, step).map(|_| None))),
            Message::Join => {
                let login = self.setup.as_ref().and_then(|s| s.login.clone()).unwrap_or_default();
                self.run("Getting ready", move |sx, step| Outcome::SetUp(sync::join(sx, &login, step).map(Some)))
            }
            Message::SetApp(computer, app, on) => {
                self.data.set_app(&computer, &app, on);
                self.save();
                Task::none()
            }
            Message::StopApp(app) => {
                sync::toggle(&mut self.data.apps, &app, false);
                self.save();
                Task::none()
            }
            Message::AddApps(apps) => {
                for a in &apps {
                    sync::toggle(&mut self.data.apps, a, true);
                }
                if let Some(ov) = &mut self.ov {
                    ov.apps.local_only.retain(|a| !apps.contains(a));
                }
                self.save();
                Task::none()
            }
            Message::SetFolder(path, on) => {
                sync::toggle(&mut self.data.folders, path, on);
                self.save();
                self.run(if on { "Adding" } else { "Removing" }, move |sx, _| Outcome::Done(sync::track(sx, path, on)))
            }
            Message::FolderHere(path, on) => {
                let host = self.sx.host.clone();
                self.data.set_folder_here(&host, path, on);
                self.save();
                Task::none()
            }
        }
    }

    /// 1, 2, 3 switch tabs; Enter presses the main button on screen.
    // iced 0.14 has no Tab focus for buttons or switches, so these are the keyboard route.
    fn shortcut(&mut self, key: Key, modifiers: keyboard::Modifiers) -> Task<Message> {
        if modifiers.control() || modifiers.alt() || modifiers.logo() || self.busy.is_some() {
            return Task::none();
        }
        let tab = match key.as_ref() {
            Key::Character("1") => Some(Tab::Sync),
            Key::Character("2") => Some(Tab::Apps),
            Key::Character("3") => Some(Tab::Folders),
            _ => None,
        };
        if let Some(t) = tab.filter(|_| self.ready()) {
            return self.update(Message::Tab(t));
        }
        if key.as_ref() != Key::Named(Named::Enter) {
            return Task::none();
        }
        let action = match &self.setup {
            Some(s) if !s.ready => Some(self.setup_action(s).1),
            Some(_) if self.tab == Tab::Sync && self.conflict.is_none() => Some(Message::Sync(Prefer::Ask)),
            _ => None,
        };
        action.map_or(Task::none(), |m| self.update(m))
    }

    fn finish(&mut self, outcome: Outcome) -> Task<Message> {
        match outcome {
            Outcome::Probed(s) => {
                let ready = s.ready;
                self.setup = Some(s);
                if ready {
                    self.data = self.sx.load_data();
                    return self.refresh();
                }
            }
            Outcome::Refreshed(Ok(ov)) => {
                self.ov = Some(ov);
                self.data = self.sx.load_data();
            }
            Outcome::Refreshed(Err(e)) => self.fail(&e),
            Outcome::Synced(Ok(notes)) => {
                let text = std::iter::once("Synced.".to_string()).chain(notes).collect::<Vec<_>>().join(" ");
                self.notice = Some(Notice { good: true, text, details: None });
                self.conflict = None;
                self.pending = false;
                return self.refresh();
            }
            Outcome::Synced(Err(SyncError::Conflict(files))) => self.conflict = Some(files),
            Outcome::Synced(Err(SyncError::Failed(e))) => {
                self.fail(&e);
                return self.refresh();
            }
            Outcome::SetUp(Ok(backup)) => {
                if let Some(s) = &mut self.setup {
                    s.ready = true;
                }
                self.data = self.sx.load_data();
                let task = self.update(Message::Sync(Prefer::Ask));
                if let Some(b) = backup {
                    let text = format!("Your previous settings were backed up to {}.", b.display());
                    self.notice = Some(Notice { good: true, text, details: None });
                }
                return task;
            }
            Outcome::SetUp(Err(e)) => self.fail(&e),
            Outcome::Done(r) => {
                if let Err(e) = r {
                    self.fail(&e);
                }
                return if self.ready() { self.refresh() } else { self.probe() };
            }
        }
        Task::none()
    }
}

// ---------- Views ----------

impl App {
    fn view(&self) -> Element<'_, Message> {
        let body: Element<Message> = match &self.setup {
            None => self.busy_line("Getting ready"),
            Some(s) if !s.ready => self.setup_view(s),
            Some(_) => self.main_view(),
        };
        let footer = self.small("Keyboard: 1, 2 and 3 switch tabs. Enter presses the main button.", self.p.quiet);
        let page = column![body, space().height(40), footer].max_width(660);
        let page = container(page).center_x(Length::Fill).padding([32, 40]);
        container(scrollable(page).height(Length::Fill)).style(self.p.page()).width(Length::Fill).height(Length::Fill).into()
    }

    // Text helpers keep sizes and fonts consistent everywhere.
    fn heading(&self, s: impl text::IntoFragment<'static>) -> Element<'static, Message> {
        text(s).size(31).font(self.head).line_height(1.2).into()
    }

    fn title(&self, s: impl text::IntoFragment<'static>, color: iced::Color) -> Element<'static, Message> {
        text(s).size(18).font(self.head).color(color).into()
    }

    fn body(&self, s: impl text::IntoFragment<'static>, color: iced::Color) -> Element<'static, Message> {
        text(s).size(15).color(color).into()
    }

    fn small(&self, s: impl text::IntoFragment<'static>, color: iced::Color) -> Element<'static, Message> {
        text(s).size(13).color(color).into()
    }

    fn primary(&self, label: &'static str, msg: Option<Message>) -> Element<'static, Message> {
        button(text(label).size(17).font(self.head)).padding([13, 26]).style(self.p.primary()).on_press_maybe(msg).into()
    }

    fn secondary(&self, label: impl text::IntoFragment<'static>, msg: Message) -> Element<'static, Message> {
        button(text(label).size(15)).padding([10, 18]).style(self.p.secondary()).on_press_maybe(self.busy.is_none().then_some(msg)).into()
    }

    fn link(&self, label: impl text::IntoFragment<'static>, msg: Message) -> Element<'static, Message> {
        button(text(label).size(13)).padding(0).style(self.p.link(self.p.quiet)).on_press_maybe(self.busy.is_none().then_some(msg)).into()
    }

    fn switch(&self, on: bool, msg: impl Fn(bool) -> Message + 'static) -> Element<'static, Message> {
        toggler(on).size(24).style(self.p.switch()).on_toggle_maybe(self.busy.is_none().then_some(msg)).into()
    }

    fn section(&self, label: &'static str) -> Element<'static, Message> {
        column![space().height(26), self.title(label, self.p.ink), rule::horizontal(1).style(self.p.hairline()), space().height(2)]
            .spacing(8)
            .into()
    }

    /// The accent dot that breathes while work is running.
    fn pulse_dot(&self, size: f32) -> Element<'static, Message> {
        let alpha = 0.35 + 0.65 * (0.5 + 0.5 * self.pulse.sin());
        container(space()).width(size).height(size).style(self.p.fill(self.p.accent.scale_alpha(alpha), size / 2.0)).into()
    }

    fn busy_line(&self, step: &str) -> Element<'static, Message> {
        row![self.pulse_dot(10.0), self.body(format!("{step}…"), self.p.ink)].spacing(12).align_y(Alignment::Center).into()
    }

    fn notice_view(&self) -> Option<Element<'static, Message>> {
        let n = self.notice.as_ref()?;
        let dot = container(space()).width(8).height(8).style(self.p.fill(if n.good { self.p.good } else { self.p.bad }, 4.0));
        let mut col = Column::new().spacing(6).push(row![dot, self.body(n.text.clone(), self.p.ink)].spacing(10).align_y(Alignment::Center));
        if let Some(d) = &n.details {
            // Hidden until asked for; it's for whoever helps fix the problem.
            let extra: Element<Message> = if self.show_details {
                text(d.clone()).size(12).font(Font::MONOSPACE).color(self.p.quiet).into()
            } else {
                row![space().width(18), self.link("Show details", Message::ShowDetails)].into()
            };
            col = col.push(extra);
        }
        Some(column![space().height(14), col].into())
    }

    // ---------- First run ----------

    /// The button for the current setup step.
    fn setup_action(&self, s: &SetupState) -> (&'static str, Message) {
        if !s.missing.is_empty() {
            ("Install helpers", Message::InstallHelpers)
        } else if !s.signed_in && self.signing_in {
            ("I've signed in", Message::SignedIn)
        } else if !s.signed_in {
            ("Sign in to GitHub", Message::SignIn)
        } else if s.found {
            ("Use my synced settings", Message::Join)
        } else {
            ("Start syncing", Message::Start)
        }
    }

    fn setup_view(&self, s: &SetupState) -> Element<'_, Message> {
        let p = self.p;
        let current = if !s.missing.is_empty() { 0 } else if !s.signed_in { 1 } else { 2 };
        let last = if s.found { "Bring in your synced settings" } else { "Save this computer's settings" };
        let steps = ["Install the sync helpers", "Sign in to GitHub", last].into_iter().enumerate().map(|(i, label)| {
            let done = i < current;
            let (fill, ink) = if done { (p.good, p.bg) } else if i == current { (p.accent, p.bg) } else { (p.raised, p.quiet) };
            let mark = if done { "✓".to_string() } else { (i + 1).to_string() };
            let badge = container(text(mark).size(14).font(self.head).color(ink)).center(28).style(p.fill(fill, 14.0));
            let label = if i == current { self.title(label, p.ink) } else { self.body(label, p.quiet) };
            row![badge, label].spacing(14).align_y(Alignment::Center).into()
        });

        let hint = if !s.missing.is_empty() {
            format!("Installs {}. Omarchy will ask for your password.", s.missing.join(" and "))
        } else if !s.signed_in && self.signing_in {
            "Finish signing in in the window that opened, then come back here.".into()
        } else if !s.signed_in {
            "A window opens with a short code to confirm in your browser.".into()
        } else if s.found {
            "This computer's settings are replaced by your synced ones. A backup is saved first.".into()
        } else {
            "This is your first computer. Its settings become the starting point for the others.".into()
        };

        let action: Element<Message> = match &self.busy {
            Some(step) => self.busy_line(step),
            None => {
                let (label, msg) = self.setup_action(s);
                let mut r = row![self.primary(label, Some(msg))].spacing(20).align_y(Alignment::Center);
                if self.signing_in && !s.signed_in {
                    r = r.push(self.link("Open the sign-in window again", Message::SignIn));
                }
                r.into()
            }
        };

        let mut col = column![
            space().height(20),
            self.heading("Keep your computers in step"),
            self.body("Your Omarchy settings, themes, plugins and apps, the same on every computer. They're kept privately in your GitHub account.", p.quiet),
            space().height(18),
            Column::with_children(steps).spacing(14),
            space().height(22),
            self.body(hint, p.quiet),
            action,
        ]
        .spacing(10);
        if let Some(n) = self.notice_view() {
            col = col.push(n);
        }
        col.into()
    }

    // ---------- Main window ----------

    fn main_view(&self) -> Element<'_, Message> {
        let p = self.p;
        let tabs = [(Tab::Sync, "Sync"), (Tab::Apps, "Apps"), (Tab::Folders, "What's synced")].map(|(tab, name)| {
            let on = self.tab == tab;
            let mark = container(space()).width(Length::Fill).height(2).style(p.fill(if on { p.accent } else { iced::Color::TRANSPARENT }, 1.0));
            let label = column![text(name).size(18).font(self.head), mark].spacing(6).width(Length::Shrink);
            button(label).padding(0).style(p.link(if on { p.ink } else { p.quiet })).on_press(Message::Tab(tab)).into()
        });
        let mut col = column![row(tabs).spacing(26), space().height(26)];

        if self.pending && self.busy.is_none() && self.tab != Tab::Sync {
            col = col.push(
                row![
                    self.body("Your other computers get this change the next time you sync.", p.warn),
                    space::horizontal(),
                    self.secondary("Sync now", Message::Sync(Prefer::Ask))
                ]
                .spacing(16)
                .align_y(Alignment::Center),
            );
            col = col.push(space().height(18));
        }

        col.push(match self.tab {
            Tab::Sync => self.sync_view(),
            Tab::Apps => self.apps_view(),
            Tab::Folders => self.folders_view(),
        })
        .into()
    }

    fn headline(&self) -> (String, String) {
        if let Some(step) = &self.busy {
            return (format!("{step}…"), "You can keep working while this runs.".into());
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

    fn sync_view(&self) -> Element<'_, Message> {
        let p = self.p;
        let (head, detail) = self.headline();
        let mut headline = row![].spacing(16).align_y(Alignment::Center);
        if self.busy.is_some() {
            headline = headline.push(self.pulse_dot(14.0));
        }
        let mut col = column![headline.push(self.heading(head))].spacing(10);
        if !detail.is_empty() {
            col = col.push(self.body(detail, p.quiet));
        }
        col = col.push(space().height(12));

        if let Some(files) = &self.conflict {
            for name in labels(files).into_keys() {
                col = col.push(self.body(format!("•  {name}"), p.ink));
            }
            col = col.push(space().height(6));
            col = col.push(
                row![
                    self.secondary("Keep this computer's version", Message::Sync(Prefer::ThisComputer)),
                    self.secondary("Keep the other computer's version", Message::Sync(Prefer::OtherComputer))
                ]
                .spacing(12),
            );
        } else {
            col = col.push(self.primary("Sync now", self.busy.is_none().then_some(Message::Sync(Prefer::Ask))));
        }
        if let Some(n) = self.notice_view() {
            col = col.push(n);
        }

        let Some(ov) = &self.ov else { return col.into() };
        if !ov.unshared.is_empty() {
            col = col.push(self.section("Not shared yet"));
            for (name, files) in labels(&ov.unshared) {
                let mut entry = column![self.body(name, p.ink)].spacing(2);
                if !files.is_empty() {
                    let mut list = files.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
                    if files.len() > 5 {
                        list = format!("{list} and {} more", files.len() - 5);
                    }
                    entry = entry.push(self.small(list, p.quiet));
                }
                col = col.push(entry);
            }
        }

        col = col.push(self.section("Your computers"));
        for (name, when) in &ov.computers {
            let here = *name == self.sx.host;
            let name = if here { self.title(name.clone(), p.ink) } else { self.body(name.clone(), p.ink) };
            let mut r = row![name].spacing(10).align_y(Alignment::Center);
            if here {
                r = r.push(self.body("this computer", p.quiet));
            }
            let when = when.map_or("hasn't synced yet".to_string(), |t| format!("synced {}", sync::ago(sync::now() - t)));
            col = col.push(r.push(space::horizontal()).push(self.body(when, p.quiet)));
        }
        col.into()
    }

    fn apps_view(&self) -> Element<'_, Message> {
        let p = self.p;
        let host = self.sx.host.clone();
        let apps = self.ov.as_ref().map(|o| o.apps.clone()).unwrap_or_default();
        let mut computers: Vec<String> = self.data.computers.keys().filter(|c| **c != host).cloned().collect();
        computers.insert(0, host.clone());
        const CELL: f32 = 112.0;

        let mut col = column![
            self.title("Synced apps", p.ink),
            self.body("A switched-on app is installed on that computer when it syncs. Switching off never uninstalls anything.", p.quiet),
            space().height(10),
        ]
        .spacing(6);

        if self.data.apps.is_empty() {
            col = col.push(self.body("No apps are synced yet. Pick some from the list below.", p.quiet));
        } else {
            let mut header = row![container(self.small("App", p.quiet)).width(Length::Fill)];
            for c in &computers {
                let name = if *c == host { "This computer".to_string() } else { c.clone() };
                header = header.push(container(self.small(name, p.quiet)).width(CELL));
            }
            col = col.push(header.push(space().width(96)));
            for app in &self.data.apps {
                let mut about = column![self.body(app.clone(), p.ink)].spacing(2);
                if let Some(d) = apps.about.get(app) {
                    about = about.push(self.small(d.clone(), p.quiet));
                } else if self.data.wants_app(&host, app) && !apps.installed.contains(app) {
                    about = about.push(self.small("Installs here on the next sync", p.warn));
                }
                let mut r = row![container(about).width(Length::Fill).padding(iced::Padding { right: 16.0, ..Default::default() })].align_y(Alignment::Center);
                for c in &computers {
                    let (c2, a2) = (c.clone(), app.clone());
                    r = r.push(container(self.switch(self.data.wants_app(c, app), move |on| Message::SetApp(c2.clone(), a2.clone(), on))).width(CELL));
                }
                r = r.push(container(self.link("Stop syncing", Message::StopApp(app.clone()))).width(96));
                col = col.push(container(r).padding([10, 0]));
            }
        }

        col = col.push(self.section("Installed only on this computer"));
        if apps.local_only.is_empty() {
            col = col.push(self.body("Nothing here. Every app you installed yourself is synced.", p.quiet));
        } else {
            col = col.push(
                row![
                    self.body("Apps you installed here that your other computers don't have.", p.quiet),
                    space::horizontal(),
                    self.secondary(format!("Sync all {}", apps.local_only.len()), Message::AddApps(apps.local_only.clone())),
                ]
                .spacing(16)
                .align_y(Alignment::Center),
            );
            for app in &apps.local_only {
                let mut about = column![self.body(app.clone(), p.ink)].spacing(2);
                if let Some(d) = apps.about.get(app) {
                    about = about.push(self.small(d.clone(), p.quiet));
                }
                let add = self.secondary("Sync to all computers", Message::AddApps(vec![app.clone()]));
                col = col.push(container(row![container(about).width(Length::Fill), add].spacing(16).align_y(Alignment::Center)).padding([8, 0]));
            }
        }
        col.into()
    }

    fn folders_view(&self) -> Element<'_, Message> {
        let p = self.p;
        let host = &self.sx.host;
        let mut col = column![self.body("Turn on what should be the same on every computer.", p.quiet), space().height(8)].spacing(4);
        for (path, name, _) in sync::FOLDERS {
            let synced = self.data.folders.iter().any(|f| f == path);
            if !synced && !self.sx.home.join(path).exists() {
                continue;
            }
            let mut about = column![self.body(*name, p.ink), self.small(format!("~/{path}"), p.quiet)].spacing(2);
            if synced {
                let here = self.data.uses_folder(host, path);
                let mut extra = row![].spacing(10);
                if !here {
                    extra = extra.push(self.small("This computer keeps its own version.", p.warn));
                }
                let label = if here { "Keep a separate version on this computer" } else { "Use the synced version" };
                about = about.push(space().height(4)).push(extra.push(self.link(label, Message::FolderHere(path, !here))));
            }
            let r = row![container(about).width(Length::Fill), self.switch(synced, move |on| Message::SetFolder(path, on))].align_y(Alignment::Center);
            col = col.push(container(r).padding([12, 0]));
        }
        col.push(space().height(8))
            .push(self.small("Your display layout (monitors.lua) always stays separate on each computer, because every screen is different.", p.quiet))
            .into()
    }
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
