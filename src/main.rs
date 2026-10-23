mod style;
mod sync;

use iced::animation::Animation;
use iced::futures::{SinkExt, StreamExt};
use iced::keyboard::{self, Key, key::Named};
use iced::time::Instant;
use iced::widget::{Column, Row, button, column, container, responsive, row, rule, scrollable, space, text, toggler};
use iced::{Alignment, Color, Element, Font, Length, Subscription, Task, window};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use style::{Fonts, Palette};
use sync::{Conflict, Ctx, Data, Overview, Prefer, SetupState, SyncError};

/// Prose stays within about 80 characters at 15 px.
const PROSE: f32 = 580.0;
/// Below this width (tiled windows) the layout tightens and the apps table stacks.
const NARROW: f32 = 480.0;

fn main() -> iced::Result {
    let fonts = style::fonts();
    let (body, medium, bold) = (fonts.body, fonts.medium, fonts.bold);
    let mut app = iced::application(move || App::new(Fonts { body, medium, bold, files: vec![] }), App::update, App::view)
        .title("Omarchy Sync")
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(body)
        .window(window::Settings {
            size: iced::Size::new(780.0, 700.0),
            min_size: Some(iced::Size::new(360.0, 420.0)),
            platform_specific: window::settings::PlatformSpecific { application_id: "omarchy-sync".into(), ..Default::default() },
            ..Default::default()
        });
    for f in fonts.files {
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
    Frame(Instant),
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
    f: Fonts,
    motion: bool,
    setup: Option<SetupState>,
    tab: Tab,
    /// The step being worked on while a background job runs.
    busy: Option<String>,
    /// True while that job is a sync (the button then reads "Syncing…").
    syncing: bool,
    pulse: Animation<bool>,
    now: Instant,
    data: Data,
    ov: Option<Overview>,
    notice: Option<Notice>,
    show_details: bool,
    conflict: Option<Conflict>,
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
    fn new(f: Fonts) -> (App, Task<Message>) {
        let mut app = App {
            sx: Ctx::load(),
            p: Palette::load(),
            f,
            motion: style::motion_allowed(),
            setup: None,
            tab: Tab::Sync,
            busy: None,
            syncing: false,
            pulse: Animation::new(false),
            now: Instant::now(),
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
        if self.busy.is_some() && self.motion {
            subs.push(window::frames().map(Message::Frame));
        } else if self.busy.is_none() && self.ready() {
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
        self.now = Instant::now();
        self.pulse = Animation::new(false).slow().repeat_forever().auto_reverse().go(true, self.now);
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
                self.syncing = false;
                self.finish(outcome)
            }
            Message::Frame(now) => {
                self.now = now;
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
                if self.busy.is_some() {
                    return Task::none();
                }
                self.notice = None;
                self.tab = Tab::Sync;
                self.syncing = true;
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

    // iced 0.14 gives buttons and switches no Tab focus, so shortcuts are the keyboard route.
    /// Ctrl+Q quits; Ctrl+R or F5 syncs; 1-3 or Ctrl+1-3 switch tabs; Enter presses the main button.
    fn shortcut(&mut self, key: Key, m: keyboard::Modifiers) -> Task<Message> {
        if m.alt() || m.logo() {
            return Task::none();
        }
        if m.control() && key.as_ref() == Key::Character("q") {
            return iced::exit();
        }
        if self.busy.is_some() {
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
        if self.conflict.is_some() && !m.control() {
            match key.as_ref() {
                Key::Character("t") => return self.update(Message::Sync(Prefer::ThisComputer)),
                Key::Character("o") => return self.update(Message::Sync(Prefer::OtherComputer)),
                _ => {}
            }
        }
        let sync_key = (m.control() && key.as_ref() == Key::Character("r")) || key.as_ref() == Key::Named(Named::F5);
        if sync_key && self.ready() && self.conflict.is_none() {
            return self.update(Message::Sync(Prefer::Ask));
        }
        if m.control() || key.as_ref() != Key::Named(Named::Enter) {
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
            Outcome::Synced(Err(SyncError::Conflict(c))) => self.conflict = Some(c),
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

// ---------- Building blocks ----------

impl App {
    fn view(&self) -> Element<'_, Message> {
        responsive(move |size| {
            let narrow = size.width < NARROW;
            let body: Element<Message> = match &self.setup {
                None => self.busy_line("Getting ready"),
                Some(s) if !s.ready => self.setup_view(s),
                Some(_) => self.main_view(narrow),
            };
            let keys = if self.conflict.is_some() {
                "Keyboard: T keeps this computer's version, O keeps the other one. Ctrl+Q quits."
            } else if self.ready() {
                "Keyboard: 1, 2 and 3 switch tabs. Enter or Ctrl+R syncs. Ctrl+Q quits."
            } else {
                "Keyboard: Enter presses the main button. Ctrl+Q quits."
            };
            let page = column![body, space().height(40), self.small(keys, self.p.quiet)].max_width(660);
            let page = container(page).center_x(Length::Fill).padding([32, if narrow { 16 } else { 40 }]);
            container(scrollable(page).height(Length::Fill)).style(self.p.page()).width(Length::Fill).height(Length::Fill).into()
        })
        .into()
    }

    fn headline_text(&self, s: impl text::IntoFragment<'static>, narrow: bool) -> Element<'static, Message> {
        text(s).size(if narrow { 24 } else { 30 }).font(self.f.medium).line_height(1.2).into()
    }

    fn title(&self, s: impl text::IntoFragment<'static>, color: Color) -> Element<'static, Message> {
        text(s).size(17).font(self.f.bold).color(color).into()
    }

    fn body(&self, s: impl text::IntoFragment<'static>, color: Color) -> Element<'static, Message> {
        text(s).size(15).line_height(1.4).color(color).into()
    }

    /// Wrapping paragraphs are capped near 80 characters per line.
    fn prose(&self, s: impl text::IntoFragment<'static>, color: Color) -> Element<'static, Message> {
        container(self.body(s, color)).max_width(PROSE).into()
    }

    fn small(&self, s: impl text::IntoFragment<'static>, color: Color) -> Element<'static, Message> {
        text(s).size(13).line_height(1.4).color(color).into()
    }

    fn primary(&self, label: impl text::IntoFragment<'static>, msg: Option<Message>) -> Element<'static, Message> {
        button(text(label).size(16).font(self.f.bold)).padding([13, 28]).style(self.p.primary()).on_press_maybe(msg).into()
    }

    fn secondary(&self, label: impl text::IntoFragment<'static>, msg: Message) -> Element<'static, Message> {
        button(text(label).size(14)).padding([9, 16]).style(self.p.secondary()).on_press_maybe(self.busy.is_none().then_some(msg)).into()
    }

    /// Padding brings the click area to at least 24 px tall (WCAG 2.5.8).
    fn link(&self, label: impl text::IntoFragment<'static>, msg: Message) -> Element<'static, Message> {
        button(text(label).size(13)).padding([5, 0]).style(self.p.link(self.p.quiet)).on_press_maybe(self.busy.is_none().then_some(msg)).into()
    }

    fn switch(&self, on: bool, msg: impl Fn(bool) -> Message + 'static) -> Element<'static, Message> {
        toggler(on).size(24).style(self.p.switch()).on_toggle_maybe(self.busy.is_none().then_some(msg)).into()
    }

    /// A titled group: heading and optional caption above a tinted card of rows with separators.
    fn group(&self, first: bool, title: &'static str, caption: Option<&'static str>, rows: Vec<Element<'static, Message>>) -> Element<'static, Message> {
        let mut head = column![self.title(title, self.p.ink)].spacing(4);
        if let Some(c) = caption {
            head = head.push(self.prose(c, self.p.quiet));
        }
        let count = rows.len();
        let mut card = Column::new();
        for (i, r) in rows.into_iter().enumerate() {
            card = card.push(container(r).padding([12, 18]).width(Length::Fill));
            if i + 1 < count {
                card = card.push(container(rule::horizontal(1).style(self.p.separator())).padding([0, 18]));
            }
        }
        column![space().height(if first { 0 } else { 28 }), head, space().height(4), container(card).style(self.p.card()).width(Length::Fill)].spacing(6).into()
    }

    /// Name on the left, details under it, a control on the right.
    fn item(&self, name: impl text::IntoFragment<'static>, detail: Option<(String, Color)>, end: Option<Element<'static, Message>>) -> Element<'static, Message> {
        let mut left = column![self.body(name, self.p.ink)].spacing(1);
        if let Some((d, c)) = detail {
            left = left.push(self.small(d, c));
        }
        let mut r = row![container(left).width(Length::Fill)].spacing(16).align_y(Alignment::Center);
        if let Some(e) = end {
            r = r.push(e);
        }
        r.into()
    }

    /// The status dot breathes while work runs, unless the desktop asks for reduced motion.
    fn dot(&self, size: f32, color: Color) -> Element<'static, Message> {
        let alpha = if self.busy.is_some() && self.motion { self.pulse.interpolate(0.3, 1.0, self.now) } else { 1.0 };
        container(space()).width(size).height(size).style(self.p.fill(color.scale_alpha(alpha), size / 2.0)).into()
    }

    fn busy_line(&self, step: &str) -> Element<'static, Message> {
        row![self.dot(10.0, self.p.accent_mark), self.body(format!("{step}…"), self.p.ink)].spacing(12).align_y(Alignment::Center).into()
    }

    fn notice_view(&self) -> Option<Element<'static, Message>> {
        let n = self.notice.as_ref()?;
        let dot = container(space()).width(8).height(8).style(self.p.fill(if n.good { self.p.good } else { self.p.bad }, 4.0));
        let mut col = Column::new().spacing(4).push(row![dot, self.prose(n.text.clone(), self.p.ink)].spacing(10).align_y(Alignment::Center));
        if let Some(d) = &n.details {
            // Hidden until asked for; it's for whoever helps fix the problem.
            let extra: Element<Message> = if self.show_details {
                container(text(d.clone()).size(12).font(Font::MONOSPACE).color(self.p.quiet)).padding([4, 18]).into()
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

    fn setup_view(&self, s: &SetupState) -> Element<'static, Message> {
        let p = self.p;
        let current = if !s.missing.is_empty() { 0 } else if !s.signed_in { 1 } else { 2 };
        let last = if s.found { "Bring in your synced settings" } else { "Save this computer's settings" };
        let steps = ["Install the sync helpers", "Sign in to GitHub", last].into_iter().enumerate().map(|(i, label)| {
            let done = i < current;
            let (fill, ink) = if done { (p.good, p.bg) } else if i == current { (p.accent, p.on_accent) } else { (p.raised, p.quiet) };
            let mark = if done { "✓".to_string() } else { (i + 1).to_string() };
            let badge = container(text(mark).size(14).font(self.f.bold).color(ink)).center(28).style(p.fill(fill, 14.0));
            let label = if i == current { self.title(label, p.ink) } else { self.body(label, p.quiet) };
            row![badge, label].spacing(14).align_y(Alignment::Center).into()
        });

        // Each step says why it's needed, not just what it does.
        let why = if !s.missing.is_empty() {
            "Omarchy Sync needs a couple of small helper tools to copy your settings safely. Omarchy will ask for your password once.".into()
        } else if !s.signed_in && self.signing_in {
            "Finish signing in in the window that opened, then come back here.".into()
        } else if !s.signed_in {
            "Your settings are stored in a private space in your GitHub account that only you can see. Signing in happens in your browser.".into()
        } else if s.found {
            "We found settings from your other computers. They replace this computer's settings, and a backup of the current ones is saved first.".into()
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
            space().height(12),
            self.small(format!("Step {} of 3", current + 1), p.quiet),
            self.headline_text("Keep your computers in step", false),
            self.prose("Your Omarchy settings, themes, plugins and apps, the same on every computer.", p.quiet),
            space().height(16),
            Column::with_children(steps).spacing(14),
            space().height(20),
            self.prose(why, p.ink),
            space().height(4),
            action,
        ]
        .spacing(8);
        if let Some(n) = self.notice_view() {
            col = col.push(n);
        }
        col.into()
    }

    // ---------- Main window ----------

    fn main_view(&self, narrow: bool) -> Element<'static, Message> {
        let p = self.p;
        let tabs = [(Tab::Sync, "Sync"), (Tab::Apps, "Apps"), (Tab::Folders, "What's synced")].map(|(tab, name)| {
            let on = self.tab == tab;
            let mark = container(space()).width(Length::Fill).height(3).style(p.fill(if on { p.accent_mark } else { Color::TRANSPARENT }, 1.5));
            let label = column![text(name).size(16).font(self.f.bold), mark].spacing(7).width(Length::Shrink);
            button(label).padding([4, 0]).style(p.link(if on { p.ink } else { p.quiet })).on_press(Message::Tab(tab)).into()
        });
        let mut col = column![row(tabs).spacing(26), space().height(22)];

        if self.pending && self.busy.is_none() && self.tab != Tab::Sync {
            let banner = row![
                container(self.body("Your other computers get this change the next time you sync.", p.ink)).width(Length::Fill),
                self.secondary("Sync now", Message::Sync(Prefer::Ask))
            ]
            .spacing(16)
            .align_y(Alignment::Center);
            col = col.push(container(banner).padding([10, 18]).style(p.card()));
            col = col.push(space().height(24));
        }

        col.push(match self.tab {
            Tab::Sync => self.sync_view(narrow),
            Tab::Apps => self.apps_view(narrow),
            Tab::Folders => self.folders_view(),
        })
        .into()
    }

    /// One fixed set of states, each with a headline, a detail line and a dot colour.
    fn status(&self) -> (String, String, Color) {
        let p = self.p;
        if let Some(step) = &self.busy {
            return (format!("{step}…"), "You can keep working while this runs.".into(), p.accent_mark);
        }
        if self.conflict.is_some() {
            return (
                "The same setting was changed on two computers.".into(),
                "Choose which version to keep, then syncing carries on.".into(),
                p.warn,
            );
        }
        let Some(ov) = &self.ov else { return ("Checking for changes…".into(), String::new(), p.accent_mark) };
        let last = match ov.computers.iter().find(|(n, _)| *n == self.sx.host).and_then(|(_, t)| *t) {
            Some(t) => format!("Last synced {}.", sync::ago(sync::now() - t)),
            None => "This computer hasn't synced yet.".to_string(),
        };
        if !ov.online {
            let detail = format!("You seem to be offline. Your changes are kept here until you reconnect. {last}");
            return ("Can't reach your other computers right now.".into(), detail, p.bad);
        }
        let n = ov.unshared.len();
        let changed = sync::plural(n, "change");
        let updates = sync::plural(ov.incoming, "update");
        match (n, ov.incoming) {
            (0, 0) => ("Everything matches your other computers.".into(), last, p.good),
            (_, 0) => (format!("{changed} on this computer {} shared yet.", if n == 1 { "isn't" } else { "aren't" }), last, p.warn),
            (0, _) => (format!("Your other computers have {updates} for this one."), last, p.warn),
            _ => (format!("{changed} here and {updates} from your other computers are waiting."), last, p.warn),
        }
    }

    fn sync_view(&self, narrow: bool) -> Element<'static, Message> {
        let p = self.p;
        let (head, detail, color) = self.status();
        // The dot sits on the first line of the headline; the detail lines up with the text.
        let headline = row![container(self.dot(12.0, color)).padding([if narrow { 11 } else { 13 }, 0]), self.headline_text(head, narrow)].spacing(14);
        let mut col = column![headline].spacing(8);
        if !detail.is_empty() {
            col = col.push(row![space().width(26), self.prose(detail, p.quiet)]);
        }
        col = col.push(space().height(14));
        col = col.push(match &self.conflict {
            Some(c) => self.conflict_view(c),
            None => {
                let label = if self.syncing { "Syncing…" } else { "Sync now" };
                self.primary(label, self.busy.is_none().then_some(Message::Sync(Prefer::Ask)))
            }
        });
        if let Some(n) = self.notice_view() {
            col = col.push(n);
        }
        if let Some(ov) = self.ov.as_ref().filter(|_| self.conflict.is_none()) {
            col = col.push(self.unshared_group(ov));
            col = col.push(self.computers_group(ov));
        }
        col.into()
    }

    fn conflict_view(&self, c: &Conflict) -> Element<'static, Message> {
        let p = self.p;
        let (other, when) = match &c.by {
            Some((name, t)) => (format!("Keep {name}'s version"), format!("Changed there {}", sync::ago(sync::now() - t))),
            None => ("Keep the other computer's version".into(), "Changed on another computer".into()),
        };
        let names: Vec<Element<Message>> = labels(&c.files).into_iter().map(|(name, files)| self.item(name, Some((files.join(", "), p.quiet)), None)).collect();
        // Two equal choices: neither is styled as the recommended one.
        let choice = |label: String, note: String, msg: Message| -> Element<'static, Message> {
            column![button(text(label).size(15).font(self.f.bold)).padding([11, 18]).style(p.secondary()).on_press(msg), self.small(note, p.quiet)]
                .spacing(6)
                .into()
        };
        column![
            container(Column::with_children(names).spacing(8)).padding([12, 18]).style(p.card()).width(Length::Fill),
            space().height(10),
            row![
                choice("Keep this computer's version".into(), "What you have here now".into(), Message::Sync(Prefer::ThisComputer)),
                choice(other, when, Message::Sync(Prefer::OtherComputer)),
            ]
            .spacing(14)
            .wrap(),
        ]
        .spacing(6)
        .into()
    }

    fn unshared_group(&self, ov: &Overview) -> Element<'static, Message> {
        let p = self.p;
        let rows: Vec<Element<Message>> = if ov.unshared.is_empty() {
            vec![self.item("Everything on this computer is shared.", None, None)]
        } else {
            labels(&ov.unshared)
                .into_iter()
                .map(|(name, files)| {
                    let mut list = files.iter().take(4).cloned().collect::<Vec<_>>().join(", ");
                    if files.len() > 4 {
                        list = format!("{list} and {} more", files.len() - 4);
                    }
                    self.item(name, (!list.is_empty()).then_some((list, p.quiet)), None)
                })
                .collect()
        };
        self.group(false, "Not shared yet", None, rows)
    }

    fn computers_group(&self, ov: &Overview) -> Element<'static, Message> {
        let p = self.p;
        let mut rows: Vec<Element<Message>> = ov
            .computers
            .iter()
            .map(|(name, when)| {
                let here = *name == self.sx.host;
                // Only what's known: when it last shared changes, never an "online" guess.
                let age = when.map(|t| sync::now() - t);
                let stale = age.is_some_and(|a| a > 30 * 86400);
                let (status, color) = match age {
                    None => ("Hasn't shared anything yet".to_string(), p.quiet),
                    Some(a) => (format!("Last shared changes {}", sync::ago(a)), if stale { p.warn } else { p.quiet }),
                };
                let label = if here { format!("{name} (this computer)") } else { name.clone() };
                let font = if here { self.f.bold } else { self.f.body };
                let name: Element<Message> = text(label).size(15).font(font).color(if stale { p.quiet } else { p.ink }).into();
                row![container(name).width(Length::Fill), self.small(status, color)].spacing(16).align_y(Alignment::Center).into()
            })
            .collect();
        if ov.computers.len() < 2 {
            let how = "Install Omarchy Sync there and choose \"Use my synced settings\".".to_string();
            rows.push(self.item("Add another computer", Some((how, p.quiet)), None));
        }
        self.group(false, "Your computers", None, rows)
    }

    fn apps_view(&self, narrow: bool) -> Element<'static, Message> {
        let p = self.p;
        let host = self.sx.host.clone();
        let apps = self.ov.as_ref().map(|o| o.apps.clone()).unwrap_or_default();
        let mut computers: Vec<String> = self.data.computers.keys().filter(|c| **c != host).cloned().collect();
        computers.insert(0, host.clone());
        let label = |c: &String| if *c == host { "This computer".to_string() } else { c.clone() };
        const CELL: f32 = 112.0;

        let mut rows: Vec<Element<Message>> = Vec::new();
        if self.data.apps.is_empty() {
            rows.push(self.item("No apps are synced yet.", Some(("Pick some from the list below.".into(), p.quiet)), None));
        } else if !narrow {
            let mut header = Row::new().push(container(self.small("App", p.quiet)).width(Length::Fill));
            for c in &computers {
                header = header.push(container(self.small(label(c), p.quiet)).width(CELL));
            }
            rows.push(header.push(space().width(92)).into());
        }
        for app in &self.data.apps {
            let detail = match apps.about.get(app) {
                Some(d) => Some((d.clone(), p.quiet)),
                None if self.data.wants_app(&host, app) && !apps.installed.contains(app) => Some(("Installs here on the next sync".into(), p.warn)),
                None => None,
            };
            let stop = self.link("Stop syncing", Message::StopApp(app.clone()));
            let mut about = column![self.body(app.clone(), p.ink)].spacing(1);
            if let Some((d, c)) = detail {
                about = about.push(self.small(d, c));
            }
            let switch_for = |c: &String| {
                let (c2, a2) = (c.clone(), app.clone());
                self.switch(self.data.wants_app(c, app), move |on| Message::SetApp(c2.clone(), a2.clone(), on))
            };
            rows.push(if narrow {
                // Narrow tiles: each computer gets its own labelled line under the app.
                let mut col = column![about].spacing(8);
                for c in &computers {
                    col = col.push(row![container(self.small(label(c), p.ink)).width(Length::Fill), switch_for(c)].align_y(Alignment::Center));
                }
                col.push(stop).into()
            } else {
                let name = container(about).width(Length::Fill).padding(iced::Padding { right: 16.0, ..Default::default() });
                let mut r = Row::new().push(name).align_y(Alignment::Center);
                for c in &computers {
                    r = r.push(container(switch_for(c)).width(CELL));
                }
                r.push(container(stop).width(92)).into()
            });
        }
        let caption = "A switched-on app is installed on that computer when it syncs. Switching it off only stops that; nothing is ever uninstalled.";
        let mut col = column![self.group(true, "Synced apps", Some(caption), rows)];

        let local: Vec<Element<Message>> = if apps.local_only.is_empty() {
            vec![self.item("Every app you installed yourself is synced.", None, None)]
        } else {
            apps.local_only
                .iter()
                .map(|app| {
                    let about = apps.about.get(app).map(|d| (d.clone(), p.quiet));
                    self.item(app.clone(), about, Some(self.secondary("Sync to all", Message::AddApps(vec![app.clone()]))))
                })
                .collect()
        };
        col = col.push(self.group(false, "Installed only on this computer", Some("Apps you installed here that your other computers don't have."), local));
        if apps.local_only.len() > 1 {
            let all = self.secondary(format!("Sync all {} apps", apps.local_only.len()), Message::AddApps(apps.local_only.clone()));
            col = col.push(container(all).padding([10, 0]));
        }
        col.into()
    }

    fn folders_view(&self) -> Element<'static, Message> {
        let p = self.p;
        let host = &self.sx.host;
        let rows: Vec<Element<Message>> = sync::FOLDERS
            .iter()
            .filter(|(path, _, _)| self.data.folders.iter().any(|f| f == path) || self.sx.home.join(path).exists())
            .map(|(path, name, _)| {
                let synced = self.data.folders.iter().any(|f| f == path);
                let mut about = column![self.body(*name, p.ink), self.small(format!("~/{path}"), p.quiet)].spacing(1);
                if synced {
                    let here = self.data.uses_folder(host, path);
                    let mut extra = row![].spacing(10).align_y(Alignment::Center);
                    if !here {
                        extra = extra.push(self.small("This computer keeps its own version.", p.warn));
                    }
                    let label = if here { "Keep a separate version on this computer" } else { "Use the synced version" };
                    about = about.push(extra.push(self.link(label, Message::FolderHere(path, !here))));
                }
                let switch = self.switch(synced, move |on| Message::SetFolder(path, on));
                row![container(about).width(Length::Fill), switch].spacing(16).align_y(Alignment::Center).into()
            })
            .collect();
        column![
            self.group(true, "What's synced", Some("Turn on what should be the same on every computer."), rows),
            space().height(12),
            self.prose("Your display layout (monitors.lua) always stays separate on each computer, because every screen is different.", p.quiet),
        ]
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
