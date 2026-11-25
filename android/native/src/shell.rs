use crate::preview;
use crate::{
    MobileWindow, Page, discord_link,
    platform::{Platform, StagedFile},
    runtime::{BackgroundRuntime, BackgroundTask},
    view::{self, Destination, Frame},
};
use anyhow::{Context, Result, ensure};
use slint::ComponentHandle;
#[path = "drafts.rs"]
mod drafts;
#[path = "navigation.rs"]
mod navigation;
use std::{collections::HashMap, fs, sync::Arc};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use yapfast_discord::{
    self as discord,
    api::{Sending, Span},
    message::{self, Message, Reply},
    ready::{Access, PRIVATE},
    service::{Discord, Live, Snapshot, Status},
    token, voice_note,
};

#[path = "conversation_tools.rs"]
mod conversation_tools;

#[path = "expression_picker.rs"]
mod expression_picker;
#[path = "polls.rs"]
pub(crate) mod polls;
#[path = "room_details.rs"]
mod room_details;

#[path = "create.rs"]
mod create;

pub enum Command {
    Create(create::Event),
    Details(room_details::Event),
    Expression(expression_picker::Event),
    Poll(polls::Event),
    Filter(String),
    PickMedia,
    Tools(conversation_tools::Event),
    Navigate(String),
    Choose(String),
    Back,
    DeviceBack,
    Link(String),
    OpenDiscordQr,
    QrUnavailable(u64),
    Progress(u64, discord_link::Progress),
    Challenge,
    Profile(String, String, String),
    Presence(String),
    Pictures(u64, crate::pictures::Pictures),
    Friend(String, String),
    Web(String),
    Notification(String),
    Visibility(bool),
    Voice(String),
    VoiceLost(i64, String),
    MicState(i64, bool, String),
    ProjectionReady(i64, u32, u32),
    ProjectionEnded(i64, String),
    Draft(String, String, String),
    Reveal(String, usize),
    Send(String),
    Action(String, String),
    React(String, String),
    Media(String, i32),
    Pick,
    Record,
    Review,
    Remove,
    Cancel,
    Older,
    Request(bool),
    Picked(i64, String, Option<StagedFile>, String),
    Snapshot(u64, Arc<Snapshot>),
    Live(u64, Box<Live>),
    Reload(u64),
}
type Sender = UnboundedSender<Command>;
pub fn bind(w: &MobileWindow, tx: Sender) {
    conversation_tools::bind(w, tx.clone());
    room_details::bind(w, tx.clone());
    create::bind(w, tx.clone());
    expression_picker::bind(w, tx.clone());
    polls::bind(w, tx.clone());
    macro_rules! bind { ($method:ident, || $cmd:expr) => {{let tx=tx.clone(); w.$method(move || {let _=tx.send($cmd);});}}; ($method:ident, |$($arg:ident),*| $cmd:expr) => {{
        let tx=tx.clone(); w.$method(move |$($arg),*| {let _=tx.send($cmd);});
    }};}
    bind!(on_navigate, |s| Command::Navigate(s.into()));
    bind!(on_filter, |s| Command::Filter(s.into()));
    bind!(on_pick_media, || Command::PickMedia);
    bind!(on_choose_place, |s| Command::Choose(s.into()));
    bind!(on_back, || Command::Back);
    bind!(on_link, |s| Command::Link(s.into()));
    bind!(on_open_discord_qr, || Command::OpenDiscordQr);
    bind!(on_challenge, || Command::Challenge);
    bind!(on_save_profile, |name, pronouns, bio| Command::Profile(
        name.into(),
        pronouns.into(),
        bio.into()
    ));
    bind!(on_presence_change, |s| Command::Presence(s.into()));
    let target = w.as_weak();
    let reveal = tx.clone();
    w.on_inspect_link(move |link| {
        if let Some((message, index)) = crate::spoiler::target(&link) {
            let _ = reveal.send(Command::Reveal(message.into(), index));
            return;
        }
        let address = crate::links::masked(&link).unwrap_or(link.as_str());
        if let Some(window) = target.upgrade() {
            window.set_pending_url(address.into());
        }
    });
    bind!(on_friend_action, |user, action| Command::Friend(
        user.into(),
        action.into()
    ));
    bind!(on_web, |url| Command::Web(url.into()));
    bind!(on_voice_action, |s| Command::Voice(s.into()));
    let draft_tx = tx.clone();
    let draft_window = w.as_weak();
    w.on_change_draft(move |text, visible| {
        if let Some(w) = draft_window.upgrade() {
            let _ = draft_tx.send(Command::Draft(
                w.get_selected_room().into(),
                text.into(),
                visible.into(),
            ));
        }
    });
    let weak = w.as_weak();
    let send = tx.clone();
    w.on_send(move || {
        if let Some(window) = weak.upgrade() {
            if !window.get_sending() {
                window.set_sending(true);
                let _ = send.send(Command::Send(window.get_draft().into()));
            }
        }
    });
    bind!(on_action, |id, op| Command::Action(id.into(), op.into()));
    bind!(on_react, |id, key| Command::React(id.into(), key.into()));
    bind!(on_media, |id, i| Command::Media(id.into(), i));
    bind!(on_pick, || Command::Pick);
    bind!(on_record, || Command::Record);
    bind!(on_review, || Command::Review);
    bind!(on_unstage, || Command::Remove);
    bind!(on_clear_context, || Command::Cancel);
    bind!(on_more_history, || Command::Older);
    bind!(on_request_answer, |yes| Command::Request(yes));
}

#[derive(Default)]
struct Composer {
    body: String,
    reply: Option<String>,
    edit: Option<String>,
    before_edit: String,
    files: Vec<(String, StagedFile)>,
    note: Option<voice_note::Note>,
    nonce: Option<String>,
}
#[derive(Clone, Default)]
struct Room {
    guild: String,
    id: String,
    name: String,
    writable: bool,
    request: bool,
    callable: bool,
}
struct Shell {
    nav: navigation::Navigation,
    tools: conversation_tools::Tools,
    details: room_details::Details,
    create: create::Create,
    expressions: expression_picker::Picker,
    ui: slint::Weak<MobileWindow>,
    platform: Arc<Platform>,
    runtime: Arc<BackgroundRuntime>,
    tx: Sender,
    discord: Option<Discord>,
    snapshot: Arc<Snapshot>,
    observer: Option<BackgroundTask<()>>,
    linking: Option<BackgroundTask<()>>,
    progress: Option<BackgroundTask<()>>,
    epoch: u64,
    page: Page,
    section: i32,
    server: String,
    room: Room,
    posts: Vec<Message>,
    drafts: HashMap<String, Composer>,
    saved: drafts::Drafts,
    revealed: HashMap<String, view::Reveals>,
    preview: bool,
    samples: HashMap<String, Vec<Message>>,
    notice: String,
    phase: i32,
    code: Option<String>,
    new_qr: bool,
    open_qr_when_ready: bool,
    scanned: String,
    challenge: String,
    older: bool,
    oldest: Option<u64>,
    sending: bool,
    writing: bool,
    collapse: bool,
    tail: i32,
    focus: i32,
    pending: Option<(i64, String, bool)>,
    sequence: i64,
    call: Option<crate::call::Session>,
    notification: String,
    foreground: bool,
    profile: Option<discord::profile::OwnProfile>,
    fill_profile: bool,
    pictures: crate::pictures::Pictures,
    picture_jobs: Vec<BackgroundTask<()>>,
    last_typing: Option<(String, std::time::Instant)>,
}
pub async fn run(
    ui: slint::Weak<MobileWindow>,
    platform: Arc<Platform>,
    runtime: Arc<BackgroundRuntime>,
    tx: Sender,
    mut rx: UnboundedReceiver<Command>,
) {
    let notification = platform.notification().unwrap_or_default();
    let mut s = Shell {
        nav: Default::default(),
        tools: Default::default(),
        details: Default::default(),
        create: Default::default(),
        expressions: Default::default(),
        ui,
        platform,
        runtime,
        tx,
        discord: None,
        snapshot: Default::default(),
        observer: None,
        linking: None,
        progress: None,
        epoch: 0,
        page: Page::Welcome,
        section: 0,
        server: String::new(),
        room: Room::default(),
        posts: vec![],
        drafts: HashMap::new(),
        saved: Default::default(),
        revealed: Default::default(),
        preview: false,
        samples: HashMap::new(),
        notice: String::new(),
        phase: 0,
        code: None,
        new_qr: false,
        open_qr_when_ready: false,
        scanned: String::new(),
        challenge: String::new(),
        older: false,
        oldest: None,
        sending: false,
        writing: true,
        collapse: false,
        tail: 0,
        focus: 0,
        pending: None,
        sequence: 0,
        call: None,
        profile: None,
        fill_profile: false,
        pictures: Default::default(),
        picture_jobs: vec![],
        last_typing: None,
        notification,
        foreground: true,
    };
    s.nav.appearance = fs::read_to_string(s.theme_path()).unwrap_or_default();
    match token::load() {
        Ok(login) => {
            if let Err(e) = s.connect(login) {
                s.notice = e.to_string();
            }
        }
        Err(e) if !e.is::<token::NotLinked>() => s.notice = e.to_string(),
        _ => {}
    }
    s.draw();
    loop {
        let cmd = match s.saved.deadline {
            Some(at) => tokio::select! {
                biased;
                _ = tokio::time::sleep_until(at) => {
                    if let Err(e) = s.saved.flush() { s.notice = e.to_string(); s.draw(); }
                    continue;
                }
                cmd = rx.recv() => cmd,
            },
            None => rx.recv().await,
        };
        let Some(cmd) = cmd else { break };
        match s.handle(cmd).await {
            Ok(false) => continue,
            Err(e) => s.notice = e.to_string(),
            Ok(true) => {}
        }
        s.draw();
    }
}

impl Shell {
    fn service(&self) -> Result<Discord> {
        self.discord.clone().context("Link Discord to continue.")
    }
    fn me(&self) -> &str {
        if self.preview {
            "local"
        } else {
            self.snapshot.user_id.as_deref().unwrap_or("")
        }
    }
    fn compose(&mut self) -> &mut Composer {
        self.drafts.entry(self.room.id.clone()).or_default()
    }
    fn restore_drafts(&mut self) -> Result<()> {
        if self.saved.select(&self.me().to_owned())? {
            self.drafts = self
                .saved
                .entries
                .iter()
                .map(|(room, (body, reply))| {
                    (
                        room.clone(),
                        Composer {
                            body: body.clone(),
                            reply: reply.clone(),
                            ..Default::default()
                        },
                    )
                })
                .collect();
            self.writing = true;
        }
        Ok(())
    }

    fn remember_draft(&mut self, display: Option<&str>) {
        if let Some(c) = self.drafts.get(&self.room.id) {
            if c.edit.is_some() {
                self.saved.update(&self.room.id, &c.before_edit, None);
            } else {
                self.saved
                    .update(&self.room.id, display.unwrap_or(&c.body), c.reply.as_ref());
            }
        }
    }

    fn online(&self) -> bool {
        self.preview || self.snapshot.status == Status::Online
    }
    fn connect(&mut self, login: String) -> Result<()> {
        self.observer.take();
        let service = Discord::start(login)?;
        self.epoch += 1;
        let epoch = self.epoch;
        let tx = self.tx.clone();
        let mut states = service.snapshots();
        self.snapshot = states.borrow().clone();
        let mut live = service.live();
        let prompts = service.clone();
        self.observer=Some(self.runtime.spawn(async move {loop {
            let cmd=tokio::select! {
                s=states.changed()=>{if s.is_err(){break} Command::Snapshot(epoch,states.borrow_and_update().clone())},
                event=live.recv()=>match event {Ok(Live::MfaRequired{id,..})=>{prompts.cancel_mfa(id);continue},Ok(e)=>Command::Live(epoch,Box::new(e)),Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>Command::Reload(epoch),Err(_)=>break}
            };
            if tx.send(cmd).is_err(){break}
        }}));
        self.discord = Some(service);
        self.page = Page::Inbox;
        self.section = 0;
        self.notice.clear();
        Ok(())
    }
    fn start_link(&mut self, input: String) {
        self.cancel_link();
        self.phase = if input.is_empty() { 1 } else { 6 };
        self.notice.clear();
        let (job, mut progress) =
            discord_link::start(&self.runtime, (!input.is_empty()).then_some(input));
        self.linking = Some(job);
        let epoch = self.epoch;
        let tx = self.tx.clone();
        self.progress = Some(self.runtime.spawn(async move {
            while let Some(step) = progress.recv().await {
                if tx.send(Command::Progress(epoch, step)).is_err() {
                    break;
                }
            }
        }));
    }
    fn cancel_link(&mut self) {
        self.linking.take();
        self.progress.take();
        self.epoch += 1;
        self.phase = 0;
        self.scanned.clear();
        self.code = None;
        self.new_qr = false;
        self.open_qr_when_ready = false;
        self.challenge.clear();
    }
    fn open_discord_qr(&self) -> Result<()> {
        if self.foreground
            && let Some(url) = &self.code
        {
            self.platform.open_discord_qr(self.epoch, url)?;
        }
        Ok(())
    }
    fn cancel_pick(&mut self) {
        if self.pending.take().is_some_and(|(_, _, record)| record) {
            let _ = self.platform.simple("stopRecording");
        }
    }
    fn draw(&mut self) {
        self.sync_expressions();
        let places = self.places();
        let servers = self.guilds();
        let activity = if self.page == Page::Inbox {
            self.activities()
        } else {
            vec![]
        };
        let authors: HashMap<String, String> = if self.preview {
            self.posts
                .iter()
                .map(|m| (m.author_id.clone(), preview::avatar(&m.author_id).into()))
                .collect()
        } else if self.page == Page::Chat {
            self.discord
                .as_ref()
                .into_iter()
                .flat_map(|d| {
                    self.posts.iter().rev().take(100).map(|m| {
                        (
                            m.author_id.clone(),
                            d.avatar(Some(&self.room.guild), &m.author_id),
                        )
                    })
                })
                .collect()
        } else {
            HashMap::new()
        };
        let account_picture = if self.preview {
            preview::avatar("local").into()
        } else {
            self.snapshot
                .account
                .as_ref()
                .map_or_else(String::new, |a| a.avatar.clone())
        };
        // Limit work and allocations to the currently displayed lists.
        let room_picture = if self.preview {
            preview::avatar(&self.room.id).into()
        } else {
            self.snapshot
                .conversations
                .iter()
                .find(|c| c.id == self.room.id)
                .and_then(|c| c.avatar.clone())
                .unwrap_or_default()
        };
        let banner = self
            .profile
            .as_ref()
            .and_then(|p| p.banner.clone())
            .unwrap_or_default();
        let media: std::collections::HashSet<String> = self
            .posts
            .iter()
            .rev()
            .filter(|m| {
                !self.snapshot.blocked.contains(&m.author_id)
                    && !self.snapshot.ignored.contains(&m.author_id)
            })
            .flat_map(|m| {
                let opened = self.revealed.get(&m.id);
                m.attachments.iter().enumerate().filter_map(move |(i, f)| {
                    crate::pictures::source(f, opened.is_some_and(|r| r.files.contains(&i)))
                })
            })
            .take(16)
            .map(str::to_owned)
            .chain(std::iter::once(banner.clone()).filter(|u| !u.is_empty()))
            .collect();
        let wanted = places
            .iter()
            .chain(servers.iter().take(50))
            .chain(activity.iter())
            .take(128)
            .map(|p| p.picture.clone())
            .chain(authors.values().cloned())
            .chain(self.details.avatars())
            .chain(std::iter::once(account_picture.clone()))
            .chain(std::iter::once(room_picture.clone()))
            .chain(media.iter().cloned())
            .filter(|u| !u.is_empty() && !u.starts_with("preview://"))
            .collect::<std::collections::HashSet<_>>();
        if self.pictures.len() > 192 {
            self.pictures.retain(|u, _| wanted.contains(u));
        }
        let new: Vec<_> = wanted
            .iter()
            .filter(|u| !self.pictures.contains_key(*u))
            .cloned()
            .collect();
        if !new.is_empty()
            && let Some(service) = self.discord.clone()
        {
            for u in &new {
                self.pictures.insert(u.clone(), None);
            }
            let epoch = self.epoch;
            let tx = self.tx.clone();
            self.picture_jobs
                .retain(|task| !task.abort_handle().is_finished());
            self.picture_jobs.push(self.runtime.spawn(async move {
                for group in new.chunks(6) {
                    let mut batch = tokio::task::JoinSet::new();
                    for u in group {
                        let d = service.clone();
                        let u = u.clone();
                        let is_media = media.contains(&u);
                        batch.spawn(async move {
                            let picture = crate::pictures::fetch(&d, &u, is_media).await;
                            (u, picture)
                        });
                    }
                    let mut done = HashMap::new();
                    while let Some(Ok((u, pic))) = batch.join_next().await {
                        done.insert(u, pic);
                    }
                    if tx.send(Command::Pictures(epoch, done)).is_err() {
                        break;
                    }
                }
            }));
        }
        let account = self.snapshot.account.as_ref();
        let c = self.drafts.get(&self.room.id);
        let heading = match self.page {
            Page::Welcome => "Harmony",
            Page::Linking => "Link Discord",
            Page::Inbox => "Messages",
            Page::Channels => servers
                .iter()
                .find(|g| g.id == self.server)
                .map_or("Channels", |g| g.name.as_str()),
            Page::Notifications => "Notifications",
            Page::QuickFind => {
                if self.nav.compose_only {
                    "New message"
                } else {
                    "Search"
                }
            }
            Page::Settings => "Settings",
            Page::Requests => "Message requests",
            Page::Details => "Conversation details",
            Page::Chat => &self.room.name,
            Page::Account => "Your Profile",
            Page::Profile => "Edit profile",
            Page::Friends => "Friends",
        }
        .to_owned();
        let context = c
            .and_then(|c| {
                c.edit
                    .as_ref()
                    .map(|_| "Editing your message".to_owned())
                    .or_else(|| {
                        c.reply
                            .as_ref()
                            .and_then(|id| self.posts.iter().find(|m| m.id == *id))
                            .map(|m| format!("Replying to {}", m.author))
                    })
            })
            .unwrap_or_default();
        let mut messages = if self.page == Page::Chat {
            self.posts.clone()
        } else {
            vec![]
        };
        for m in &mut messages {
            if self.snapshot.blocked.contains(&m.author_id)
                || self.snapshot.ignored.contains(&m.author_id)
            {
                m.content = "Hidden message".into();
                m.system = None;
                m.reply = None;
                m.attachments.clear();
                m.reactions.clear();
                m.embeds.clear();
                m.poll = None;
            } else if let Some(service) = &self.discord {
                let guild = self
                    .snapshot
                    .servers
                    .iter()
                    .find(|g| g.id == self.room.guild);
                m.content = service.render(guild.map(|g| g.server.as_ref()), &m.content);
            }
        }
        let notice = if self.notice.is_empty() && !self.preview && self.discord.is_some() {
            match &self.snapshot.status {
                Status::Offline(s) => s.clone(),
                Status::Connecting => "Connecting to Discord…".into(),
                _ => String::new(),
            }
        } else {
            self.notice.clone()
        };
        let profile = if std::mem::take(&mut self.fill_profile) {
            self.profile
                .as_ref()
                .map(|p| (p.name.clone(), p.pronouns.clone(), p.bio.clone()))
        } else {
            None
        };
        let role_colors = if self.page == Page::Chat {
            self.discord
                .as_ref()
                .zip(
                    self.snapshot
                        .servers
                        .iter()
                        .find(|s| s.id == self.room.guild),
                )
                .map(|(d, g)| {
                    self.posts
                        .iter()
                        .filter_map(|m| d.color(g, &m.author_id).map(|v| (m.author_id.clone(), v)))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            HashMap::new()
        };
        let (can_pin, deletes) = self.tool_permissions();
        let f = Frame {
            composer_context: (self.page == Page::Chat).then(|| {
                crate::emoji::Context::new(
                    &self.room.id,
                    &self.room.guild,
                    &self.posts,
                    self.snapshot.clone(),
                    self.discord.clone(),
                    self.runtime.clone(),
                    self.preview,
                )
            }),
            can_poll: self.can_create_poll(),
            room_muted: self.room_muted(),
            banner,
            role_colors,
            servers,
            activity,
            server_id: self.server.clone(),
            room_id: self.room.id.clone(),
            room_name: self.room.name.clone(),
            room_picture,
            direct: self.room.guild == PRIVATE,
            appearance: self.appearance().into(),
            requests: self.request_total(),
            alerts: if self.preview {
                preview::activity()
                    .iter()
                    .filter(|d| !self.nav.closed.contains(&format!("alert/{}", d.id)))
                    .count() as i32
            } else {
                self.nav.alerts.len() as i32
            },
            topic: self
                .snapshot
                .servers
                .iter()
                .find(|s| s.id == self.room.guild)
                .and_then(|guild| {
                    let raw = guild.channel(&self.room.id)?.topic.clone()?;
                    Some(
                        self.discord
                            .as_ref()
                            .map_or_else(|| raw.to_string(), |d| d.render(Some(guild), &raw)),
                    )
                })
                .unwrap_or_default(),
            query: self.nav.query.clone(),
            can_pin,
            deletes,
            profile,
            presence: if self.preview {
                self.nav.local_presence.clone()
            } else {
                self.snapshot.presence.clone()
            },
            pictures: self.pictures.clone(),
            revealed: self.revealed.clone(),
            authors,
            account_picture,
            page: self.page,
            heading,
            name: if self.preview {
                self.nav.local_profile.name.clone()
            } else {
                account.map_or("Harmony", |a| &a.name).to_string()
            },
            handle: account.map_or_else(String::new, |a| format!("@{}", a.username)),
            me: self.me().into(),
            online: self.online(),
            preview: self.preview,
            notice,
            phase: self.phase,
            qr: if std::mem::take(&mut self.new_qr) {
                self.code.clone()
            } else {
                None
            },
            scanned: self.scanned.clone(),
            section: self.section,
            places,
            messages,
            callable: self.room.callable
                && self.room.writable
                && !self.room.request
                && !self.preview,
            call_name: self
                .call
                .as_ref()
                .map_or_else(String::new, |c| c.name.clone()),
            muted: self.call.as_ref().is_none_or(|c| c.muted),
            sharing: self.call.as_ref().is_some_and(|c| c.sharing.is_some()),
            draft: self
                .writing
                .then(|| c.map_or_else(String::new, |c| c.body.clone())),
            context,
            staged: c.map_or_else(String::new, |c| {
                c.note.as_ref().map_or_else(
                    || {
                        c.files
                            .iter()
                            .map(|(n, _)| n.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                    |n| format!("Voice message · {:.1}s", n.duration_secs()),
                )
            }),
            voice: c.is_some_and(|c| c.note.is_some()),
            recording: self.pending.as_ref().is_some_and(|(_, _, r)| *r),
            writable: self.room.writable,
            request: self.room.request,
            sending: self.sending,
            older: self.older,
            tail: self.tail,
            focus: self.focus,
            collapse: self.collapse,
        };
        self.writing = false;
        self.collapse = false;
        let _ = self.ui.upgrade_in_event_loop(move |w| f.apply(&w));
        self.sync_tools();
        self.sync_details();
        self.sync_create();
    }
    async fn handle(&mut self, cmd: Command) -> Result<bool> {
        match cmd {
            Command::DeviceBack => {
                let _ = self.ui.upgrade_in_event_loop(|w| w.invoke_go_back());
                return Ok(false);
            }
            Command::Filter(text) => self.nav.query = text,
            Command::PickMedia => self.pick(false, true)?,
            Command::Poll(event) => self.poll_event(event).await,
            Command::Expression(event) => self.expression_event(event).await?,
            Command::Back if self.expressions.open() => self.close_expressions(),
            Command::Create(event) => self.creation(event).await?,
            Command::Back if self.create.open() => self.create.back(),
            Command::Details(event) => self.details_event(event).await?,
            Command::Back if self.details.open() => self.details.back(),
            Command::Back if !self.tools.open() && self.has_detail_return() => {
                self.return_from_detail().await?
            }
            Command::Tools(event) => self.tools_event(event).await?,
            Command::Back if self.tools.open() => self.tools.close(),
            Command::Action(id, op)
                if matches!(op.as_str(), "pin" | "unpin" | "delete" | "copy") =>
            {
                self.message_tool(&id, &op).await?
            }
            Command::Presence(status) if self.preview || self.discord.is_some() => {
                ensure!(
                    ["online", "idle", "dnd", "invisible"].contains(&status.as_str()),
                    "Unknown presence."
                );
                if self.preview {
                    self.nav.local_presence = status;
                } else {
                    self.service()?.set_status(&status).await?;
                }
                self.notice.clear();
            }
            Command::Profile(name, pronouns, bio) if self.page == Page::Profile => {
                let previous = self
                    .profile
                    .as_ref()
                    .context("Reopen your profile to edit it.")?;
                ensure!(
                    name.chars().count() <= 32
                        && pronouns.chars().count() <= 40
                        && bio.chars().count() <= 190,
                    "Display name, pronouns or bio exceeds Discord’s length limit."
                );
                if self.preview {
                    self.nav.local_profile.name = if name.trim().is_empty() {
                        preview::LOCAL_NAME.to_owned()
                    } else {
                        name
                    };
                    self.nav.local_profile.pronouns = pronouns;
                    self.nav.local_profile.bio = bio;
                    self.profile = None;
                    self.page = self.nav.pages.pop().unwrap_or(Page::Account);
                    self.notice.clear();
                    return Ok(true);
                }
                let mut account = serde_json::json!({});
                let mut profile = serde_json::json!({});
                if name != previous.name {
                    account["global_name"] = if name.trim().is_empty() {
                        serde_json::Value::Null
                    } else {
                        name.trim().into()
                    };
                }
                if pronouns != previous.pronouns {
                    profile["pronouns"] = pronouns.into();
                }
                if bio != previous.bio {
                    profile["bio"] = bio.into();
                }
                self.service()?
                    .save_main_profile(&account, &profile, None)
                    .await?;
                self.profile = None;
                self.page = self.nav.pages.pop().unwrap_or(Page::Account);
                self.notice = "Profile saved.".into();
            }
            Command::Pictures(epoch, images) if self.epoch == epoch => {
                self.pictures.extend(images);
                self.create.invalidate();
                self.details.invalidate();
            }
            Command::Friend(user, action) if self.page == Page::Friends => {
                if self.preview {
                    match action.as_str() {
                        "add" => {
                            let username = user.trim();
                            ensure!(
                                !username.is_empty() && username.chars().count() <= 32,
                                "Enter a Discord username."
                            );
                            ensure!(
                                !self
                                    .people()
                                    .iter()
                                    .any(|p| p.name.eq_ignore_ascii_case(username)),
                                "This person is already listed."
                            );
                            self.nav.pending_friends.push(
                                Destination::new(
                                    &message::nonce(),
                                    username,
                                    &view::initial(username),
                                    "Outgoing request",
                                    false,
                                )
                                .request(false),
                            );
                        }
                        "remove" => self.nav.pending_friends.retain(|p| p.id != user),
                        _ => {}
                    }
                    return Ok(true);
                }
                let service = self.service()?;
                match action.as_str() {
                    "add" => {
                        service.add_friend_by_username(user.trim()).await?;
                        self.notice = "Friend request sent.".into();
                    }
                    "accept" => {
                        service.befriend(&user).await?;
                        self.notice.clear();
                    }
                    "remove" => {
                        service.remove_friend_request(&user).await?;
                        self.notice.clear();
                    }
                    _ => return Ok(false),
                }
            }
            Command::Web(url) => {
                let address = crate::links::masked(&url).unwrap_or(&url);
                ensure!(
                    address.starts_with("https://") || address.starts_with("http://"),
                    "Unsupported link."
                );
                self.platform.strings("browser", &[address])?;
                return Ok(false);
            }
            Command::Visibility(visible) => {
                self.foreground = visible;
                if !visible {
                    self.saved.flush()?;
                }
                if self.page == Page::Chat
                    && let Some(service) = &self.discord
                {
                    service.view(&self.room.guild, visible.then_some(self.room.id.as_str()));
                    if self.can_ack_visible_room() {
                        service.mark_read(&self.room.guild, &self.room.id, true);
                    }
                }
                return Ok(false);
            }
            Command::Notification(id) => {
                self.notification = id;
                self.open_notification().await?;
            }
            Command::Voice(action) => self.voice(&action).await?,
            Command::VoiceLost(id, reason)
                if self.call.as_ref().is_some_and(|c| c.serial == id) =>
            {
                self.call.take();
                self.notice = format!("Call ended. {reason}");
            }
            Command::MicState(id, on, error)
                if self.call.as_ref().is_some_and(|c| c.serial == id) =>
            {
                if let Some(c) = &mut self.call {
                    c.mute(!on)?;
                }
                self.notice = error;
            }
            Command::ProjectionReady(id, w, h)
                if self.call.as_ref().is_some_and(|c| c.sharing == Some(id)) =>
            {
                if let Some(c) = &mut self.call {
                    if let Err(e) = c.share(id, w, h).await {
                        c.stop_share();
                        return Err(e);
                    }
                }
            }
            Command::ProjectionEnded(id, why)
                if self.call.as_ref().is_some_and(|c| c.sharing == Some(id)) =>
            {
                if let Some(c) = &mut self.call {
                    c.stop_share();
                }
                self.notice = why;
            }
            Command::Navigate(to) => self.navigate(&to).await?,
            Command::Back => match self.page {
                Page::Welcome | Page::Inbox => self.platform.simple("finish")?,
                Page::Profile => {
                    self.profile = None;
                    self.page = self.nav.pages.pop().unwrap_or(Page::Account);
                    self.notice.clear();
                }
                Page::Linking => {
                    self.cancel_link();
                    self.notice.clear();
                    self.page = Page::Welcome;
                }
                Page::Chat
                    if self
                        .drafts
                        .get(&self.room.id)
                        .is_some_and(|c| c.edit.is_some() || c.reply.is_some()) =>
                {
                    self.cancel_compose()
                }
                Page::Chat => {
                    if let Some(service) = &self.discord {
                        service.view(&self.room.guild, None);
                    }
                    self.cancel_pick();
                    self.page = if self.room.guild == PRIVATE {
                        Page::Inbox
                    } else {
                        Page::Channels
                    };
                    self.notice.clear();
                }
                _ => {
                    self.page = self.nav.pages.pop().unwrap_or(Page::Inbox);
                    self.nav.query.clear();
                    self.notice.clear();
                }
            },
            Command::Link(input) => {
                if self.page == Page::Linking && matches!(self.phase, 0 | 4) {
                    self.start_link(input);
                }
            }
            Command::OpenDiscordQr if self.page == Page::Linking => {
                if self.phase == 2 {
                    self.notice.clear();
                    self.open_discord_qr()?;
                } else if matches!(self.phase, 0 | 4) {
                    self.start_link(String::new());
                    self.open_qr_when_ready = true;
                }
            }
            Command::QrUnavailable(g) if g == self.epoch && self.phase == 2 => {
                self.notice = "Discord couldn't open this link. Install or update Discord on this phone, or scan the code on another device.".into();
            }
            Command::Progress(g, step) if g == self.epoch && self.page == Page::Linking => {
                if !matches!(&step, discord_link::Progress::Code(_)) {
                    self.code = None;
                    self.new_qr = false;
                    self.open_qr_when_ready = false;
                    self.notice.clear();
                }
                match step {
                    discord_link::Progress::Code(s) => {
                        self.phase = 2;
                        self.code = Some(s);
                        self.new_qr = true;
                        if std::mem::take(&mut self.open_qr_when_ready) {
                            self.open_discord_qr()?;
                        }
                    }
                    discord_link::Progress::Scanned(n) => {
                        self.phase = 3;
                        self.scanned = n;
                    }
                    discord_link::Progress::Captcha(url) => {
                        self.phase = 5;
                        self.challenge = url;
                        self.platform.strings("browser", &[&self.challenge])?;
                    }
                    discord_link::Progress::Failed(e) => {
                        self.phase = 4;
                        self.notice = e;
                    }
                    discord_link::Progress::Expired => {
                        self.phase = 4;
                        self.notice = "Code expired. Request a new code.".into();
                    }
                    discord_link::Progress::Linked => {
                        self.cancel_link();
                        self.connect(token::load()?)?;
                    }
                }
            }
            Command::Challenge if !self.challenge.is_empty() => {
                self.platform.strings("browser", &[&self.challenge])?
            }
            Command::Choose(id) => self.choose(&id).await?,
            Command::Draft(room, s, visible) => {
                if self.page == Page::Chat && self.room.id == room {
                    let notify = !s.is_empty()
                        && self.room.writable
                        && !self.room.request
                        && self.online()
                        && self.last_typing.as_ref().is_none_or(|(room, t)| {
                            room != &self.room.id || t.elapsed().as_secs() >= 8
                        });
                    let c = self.compose();
                    if c.body != s {
                        c.body = s;
                        c.nonce = None;
                    }
                    self.remember_draft(Some(&visible));
                    if notify && let Some(service) = self.discord.clone() {
                        let room = self.room.id.clone();
                        self.last_typing = Some((room.clone(), std::time::Instant::now()));
                        tokio::spawn(async move {
                            let _ = service.typing(&room).await;
                        });
                    }
                }
                return Ok(false);
            }
            Command::Send(body) => {
                if self.page != Page::Chat {
                    return Ok(false);
                }
                if self.tools.past {
                    self.return_to_present().await?;
                }
                self.compose().body = body;
                self.remember_draft(None);
                self.send().await?;
            }
            Command::Action(id, op) => {
                let m = self
                    .posts
                    .iter()
                    .find(|m| m.id == id)
                    .context("Message no longer available")?
                    .clone();
                ensure!(self.room.writable, "This channel is read only.");
                self.cancel_compose();
                let own = m.author_id == self.me();
                let c = self.compose();
                if op == "edit" {
                    ensure!(own && m.editable, "Only your own messages can be edited.");
                    ensure!(
                        c.files.is_empty() && c.note.is_none(),
                        "Remove the attachment before editing."
                    );
                    c.before_edit = std::mem::replace(&mut c.body, m.content);
                    c.edit = Some(id);
                    self.writing = true;
                } else {
                    c.reply = Some(id);
                }
                self.collapse = true;
                self.focus += 1;
                self.remember_draft(None);
            }
            Command::Reveal(id, index) => {
                if !self.posts.iter().any(|m| m.id == id) {
                    return Ok(false);
                }
                self.revealed.entry(id).or_default().text.insert(index);
            }
            Command::React(id, key) => self.reaction(id, key).await?,
            Command::Media(id, i) => self.media(&id, i).await?,
            Command::Pick => self.pick(false, false)?,
            Command::Record if self.pending.as_ref().is_some_and(|(_, _, r)| *r) => {
                self.platform.simple("stopRecording")?
            }
            Command::Record => self.pick(true, false)?,
            Command::Remove => {
                let c = self.compose();
                c.files.clear();
                c.note = None;
                c.nonce = None;
            }
            Command::Picked(id, name, file, error) => self.picked(id, name, file, error).await?,
            Command::Cancel => self.cancel_compose(),
            Command::Review => {
                let note = self.compose().note.clone().context("No recording")?;
                self.show_media("voice-message.ogg", note.data())?;
            }
            Command::Older => {
                if let Some(before) = self.oldest {
                    self.history(Some(before)).await?;
                }
            }
            Command::Request(yes) => {
                self.service()?.answer_request(&self.room.id, yes).await?;
                self.room.request = false;
                if !yes {
                    self.page = Page::Inbox;
                }
            }
            Command::Snapshot(g, snap) if g == self.epoch => {
                let reconnected =
                    self.snapshot.status != Status::Online && snap.status == Status::Online;
                self.snapshot = self.details_snapshot(snap);
                self.restore_drafts()?;
                self.create.invalidate();
                self.details.invalidate();
                self.tools.invalidate();
                if !self.notification.is_empty() && self.open_notification().await? {
                    return Ok(true);
                }
                if self.page == Page::Chat && !self.preview {
                    if !self.update_room() {
                        self.cancel_pick();
                        self.page = Page::Inbox;
                        self.posts.clear();
                    } else if reconnected {
                        self.history(None).await?;
                    }
                }
            }
            Command::Live(g, e) if g == self.epoch => {
                if !self.live(*e)? {
                    return Ok(false);
                }
            }
            Command::Reload(g) if g == self.epoch && self.page == Page::Chat => {
                self.history(None).await?
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    async fn navigate(&mut self, to: &str) -> Result<()> {
        self.notice.clear();
        self.collapse = true;
        match to {
            "link" if self.page == Page::Welcome => {
                self.page = Page::Linking;
                self.phase = 0;
            }
            "preview" if self.page == Page::Welcome => {
                self.preview = true;
                self.restore_drafts()?;
                self.nav.local_profile = discord::profile::OwnProfile {
                    name: preview::LOCAL_NAME.into(),
                    ..Default::default()
                };
                self.nav.local_presence = "online".into();
                self.server = preview::FEATURED_SERVER.into();
                self.page = Page::Inbox;
                self.section = 0;
            }
            "profile" => {
                self.profile = Some(if self.preview {
                    self.nav.local_profile.clone()
                } else {
                    self.service()?.own_profile(None).await?
                });
                self.fill_profile = true;
                self.visit(Page::Profile);
            }
            "account" => {
                self.visit(Page::Account);
                self.profile = if self.preview {
                    Some(self.nav.local_profile.clone())
                } else {
                    self.service()?.own_profile(None).await.ok()
                };
                self.fill_profile = true;
            }
            "settings" => self.visit(Page::Settings),
            "friends" => self.visit(Page::Friends),
            "toggle-mute" if self.page == Page::Details => {
                let value = !self.room_muted();
                if self.preview {
                    let k = format!("mute/{}", self.room.id);
                    if value {
                        self.nav.closed.insert(k);
                    } else {
                        self.nav.closed.remove(&k);
                    }
                } else {
                    self.service()?
                        .change_notifications(
                            &self.room.guild,
                            Some(&self.room.id),
                            discord::notifications::Change::Mute {
                                muted: value,
                                seconds: None,
                            },
                        )
                        .await?;
                }
            }
            _ if self.page == Page::Inbox
                && (to.starts_with("inbox-read/") || to.starts_with("inbox-unread/")) =>
            {
                if let Some(id) = to.strip_prefix("inbox-read/") {
                    self.inbox_read_state(id, true).await?;
                } else if let Some(id) = to.strip_prefix("inbox-unread/") {
                    self.inbox_read_state(id, false).await?;
                }
            }
            "mark-unread" if self.page == Page::Details => {
                if self.preview {
                    self.preview_read_state(&self.room.id.clone(), false);
                } else {
                    let service = self.service()?;
                    service.view(&self.room.guild, None);
                    if self.tools.past {
                        let newest = service
                            .history(&self.room.guild, &self.room.id, Span::Latest)
                            .await?;
                        self.mark_latest_unread(&self.room.guild, &self.room.id, &newest.messages)?;
                    } else {
                        self.mark_latest_unread(&self.room.guild, &self.room.id, &self.posts)?;
                    }
                }
                self.nav.pages.clear();
                self.page = if self.room.guild == PRIVATE {
                    Page::Inbox
                } else {
                    Page::Channels
                };
            }
            "details" if self.page == Page::Chat => self.visit(Page::Details),
            "alerts" => self.visit(Page::Notifications),
            "requests" => self.visit(Page::Requests),
            "quick-find" | "compose" => {
                self.nav.compose_only = to == "compose";
                self.visit(Page::QuickFind);
            }
            "clear-alerts" => {
                self.nav.alerts.clear();
                if self.preview {
                    for d in preview::activity() {
                        self.nav.closed.insert(format!("alert/{}", d.id));
                    }
                } else {
                    self.platform.simple("clearNotifications")?;
                }
            }
            "dms" => {
                self.nav.pages.clear();
                self.page = Page::Inbox;
                self.section = 0;
            }
            "switch-area" => {
                self.nav.pages.clear();
                if self.page == Page::Channels {
                    self.page = Page::Inbox;
                } else if let Some(server) = self
                    .guilds()
                    .into_iter()
                    .find(|s| s.id == self.server)
                    .or_else(|| self.guilds().into_iter().next())
                {
                    self.server = server.id;
                    self.page = Page::Channels;
                }
            }
            _ if to.starts_with("guild/") => {
                let id = &to[6..];
                ensure!(
                    self.guilds().iter().any(|g| g.id == id),
                    "Server is no longer available."
                );
                self.nav.pages.clear();
                self.server = id.into();
                self.page = Page::Channels;
                self.section = 1;
            }
            _ if to.starts_with("chat-detail/") && self.page == Page::Details => {
                self.nav.pages.pop();
                self.page = Page::Chat;
                self.details_event(room_details::Event::Input(
                    to[12..].into(),
                    String::new(),
                    String::new(),
                ))
                .await?;
            }
            _ if to.starts_with("chat-tool/") && self.page == Page::Details => {
                self.nav.pages.pop();
                self.page = Page::Chat;
                self.tools_event(conversation_tools::Event::Input(
                    to[10..].into(),
                    String::new(),
                    String::new(),
                ))
                .await?;
            }
            _ if to.starts_with("theme/") => {
                let mode = &to[6..];
                ensure!(
                    ["light", "ash", "dark", "onyx"].contains(&mode),
                    "Unknown appearance."
                );
                let path = self.theme_path();
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let tmp = path.with_extension("tmp");
                fs::write(&tmp, mode)?;
                fs::rename(tmp, path)?;
                self.nav.appearance = mode.into();
            }
            "notifications" => self.platform.simple("notifications")?,
            "signout" => {
                self.saved.forget()?;
                self.revealed.clear();
                self.cancel_pick();
                self.call.take();
                if !self.preview {
                    token::forget()?;
                }
                self.observer.take();
                if let Some(service) = self.discord.take() {
                    tokio::spawn(async move {
                        let _ = service.logout().await;
                    });
                }
                self.snapshot = Default::default();
                self.profile = None;
                if !self.preview {
                    self.platform.simple("clearNotifications")?;
                    self.platform.simple("clearMedia")?;
                }
                self.cancel_link();
                self.picture_jobs.clear();
                self.pictures.clear();
                self.last_typing = None;
                self.preview = false;
                let appearance = std::mem::take(&mut self.nav.appearance);
                self.nav = navigation::Navigation {
                    appearance,
                    ..Default::default()
                };
                self.server.clear();
                self.create = Default::default();
                self.samples.clear();
                self.drafts.clear();
                self.posts.clear();
                self.room = Room::default();
                self.writing = true;
                self.page = Page::Welcome;
            }
            _ => {}
        }
        Ok(())
    }
    async fn choose(&mut self, id: &str) -> Result<()> {
        self.notice.clear();
        if id.starts_with("category/") && self.page == Page::Channels {
            if !self.nav.closed.remove(id) {
                self.nav.closed.insert(id.to_owned());
            }
            return Ok(());
        }
        let mut guild = if self.page == Page::Channels {
            self.server.clone()
        } else {
            PRIVATE.into()
        };
        let mut target = id.to_owned();
        let name;
        if let Some(person) = id.strip_prefix("person/") {
            let friend = self
                .people()
                .into_iter()
                .find(|p| p.id == person)
                .context("Friend no longer available")?;
            guild = PRIVATE.into();
            name = friend.name;
            target = if self.preview {
                person.into()
            } else {
                self.service()?.open_dm(person).await?
            };
        } else if let Some((g, c)) = id.strip_prefix("channel/").and_then(|s| s.split_once('/')) {
            let ch = self
                .channels(g)
                .into_iter()
                .find(|p| p.id == c)
                .context("Channel no longer available")?;
            guild = g.into();
            self.server = g.into();
            target = c.into();
            name = ch.name;
        } else if self.page == Page::Notifications {
            if self.preview {
                guild = preview::channel_server(id).unwrap_or(PRIVATE).into();
                name = preview::room_name(id).context("Unknown room")?.into();
            } else {
                self.notification = id.to_owned();
                self.nav.alerts.retain(|d| d.id != id);
                self.page = Page::Inbox;
                return Box::pin(self.open_notification()).await.map(|_| ());
            }
        } else {
            let destinations = if self.page == Page::Channels {
                self.channels(&self.server)
            } else {
                self.places()
            };
            let place = destinations
                .into_iter()
                .find(|p| p.id == id)
                .context("Conversation is no longer available")?;
            name = place.name;
            if self.page == Page::Friends && !self.preview {
                target = self.service()?.open_dm(id).await?;
            }
        }
        self.nav.pages.clear();
        if guild != PRIVATE {
            self.server = guild.clone();
        }
        self.cancel_pick();
        self.room = Room {
            guild,
            id: target,
            name,
            writable: true,
            request: false,
            callable: false,
        };
        self.update_room();
        self.page = Page::Chat;
        self.posts.clear();
        self.oldest = None;
        self.older = false;
        self.writing = true;
        self.collapse = true;
        self.tail += 1;
        if !self.preview
            && self
                .snapshot
                .servers
                .iter()
                .find(|g| g.id == self.room.guild)
                .and_then(|g| g.channel(&self.room.id))
                .is_some_and(|c| c.forum.is_some())
        {
            self.room.writable = false;
            self.service()?.view(&self.room.guild, Some(&self.room.id));
            self.details_event(room_details::Event::Input(
                "threads".into(),
                String::new(),
                String::new(),
            ))
            .await?;
            return Ok(());
        }
        if self.preview {
            if self.room.guild == PRIVATE {
                self.preview_read_state(&self.room.id.clone(), true);
            }
            self.posts = self
                .samples
                .entry(self.room.id.clone())
                .or_insert_with(|| preview::messages(&self.room.id))
                .clone();
        } else {
            let service = self.service()?;
            service.view(&self.room.guild, Some(&self.room.id));
            self.draw();
            self.history(None).await?;
            if self.foreground {
                service.mark_read(&self.room.guild, &self.room.id, true);
            }
        }
        Ok(())
    }
    fn update_room(&mut self) -> bool {
        if self.preview {
            return true;
        }
        if self.room.guild == PRIVATE {
            if let Some(c) = self
                .snapshot
                .conversations
                .iter()
                .find(|c| c.id == self.room.id)
            {
                self.room.request = c.request;
                self.room.name = c.name.clone();
                self.room.callable = !c.request;
            }
            return true;
        }
        if let Some(ch) = self
            .snapshot
            .servers
            .iter()
            .find(|g| g.id == self.room.guild)
            .and_then(|g| g.channel(&self.room.id))
        {
            self.room.callable = ch.channel_type == Some(2) && !ch.stage;
            self.room.writable = ch.marks.access == Access::Join && ch.forum.is_none();
            return ch.marks.access != Access::Hidden;
        }
        false
    }
    async fn history(&mut self, before: Option<u64>) -> Result<()> {
        let id = before.map(|v| v.to_string());
        let span = id.as_deref().map_or(Span::Latest, Span::Before);
        let mut page = self
            .service()?
            .history(&self.room.guild, &self.room.id, span)
            .await?;
        self.oldest = page.oldest;
        self.older = page.full && page.oldest.is_some();
        if before.is_none() {
            self.posts.clear();
        }
        self.posts.append(&mut page.messages);
        self.posts.sort_by_key(Message::order);
        self.posts.dedup_by(|a, b| a.id == b.id);
        if let Some(latest) = self.posts.last().cloned() {
            self.remember_message(&latest);
        }
        Ok(())
    }
    fn put(&mut self, m: Message) {
        self.remember_message(&m);
        if let Some(old) = self.posts.iter_mut().find(|p| p.id == m.id) {
            *old = m;
        } else {
            self.posts.push(m);
            self.posts.sort_by_key(Message::order);
        }
    }
    fn cancel_compose(&mut self) {
        let c = self.compose();
        c.reply = None;
        c.nonce = None;
        if c.edit.take().is_some() {
            c.body = std::mem::take(&mut c.before_edit);
            self.writing = true;
        }
        self.collapse = true;
        self.remember_draft(None);
    }
    async fn send(&mut self) -> Result<()> {
        ensure!(
            self.online() && self.room.writable && !self.room.request,
            "Reconnect to a writable conversation."
        );
        let room = self.room.clone();
        let me = self.me().to_owned();
        let mut c = self.drafts.remove(&room.id).unwrap_or_default();
        if c.body.trim().is_empty() && c.files.is_empty() && c.note.is_none() {
            self.drafts.insert(room.id, c);
            return Ok(());
        }
        let nonce = c.nonce.get_or_insert_with(message::nonce).clone();
        // Keep an in-flight draft visible and recoverable. Every retry of the same draft reuses its nonce.
        self.sending = true;
        let _ = self.ui.upgrade_in_event_loop(|w| w.set_sending(true));
        let result: Result<()> = async {
            if self.preview {
                if let Some(id) = &c.edit {
                    let m = self
                        .posts
                        .iter_mut()
                        .find(|m| m.id == *id && m.author_id == me)
                        .context("Cannot edit this message")?;
                    m.content = c.body.clone();
                    m.edited = true;
                } else {
                    let mut attachments = vec![];
                    let mut local_files = vec![];
                    if let Some(note) = &c.note {
                        let folder = self.platform.cache.join("preview-notes");
                        fs::create_dir_all(&folder)?;
                        let path = folder.join(format!("{nonce}.ogg"));
                        fs::write(&path, note.data())?;
                        local_files.push(("voice-message.ogg".to_string(), StagedFile(path)));
                    } else {
                        local_files.append(&mut c.files);
                    }
                    for (index, (name, file)) in local_files.into_iter().enumerate() {
                        let url = format!("local-attachment://{nonce}/{index}");
                        let image = matches!(
                            name.rsplit('.')
                                .next()
                                .unwrap_or("")
                                .to_lowercase()
                                .as_str(),
                            "webp" | "jpg" | "jpeg" | "png"
                        );
                        attachments.push(message::Attachment {
                            name,
                            size: file.0.metadata().ok().map(|m| m.len()),
                            url: url.clone().into(),
                            image,
                            dimensions: None,
                            preview_url: None,
                            view_url: None,
                            still_url: None,
                            video_url: None,
                            link_url: None,
                            animated: false,
                            shown: message::Shown::Message,
                            spoiler: false,
                            description: None,
                        });
                        if image && file.0.metadata().is_ok_and(|m| m.len() <= 4 * 1024 * 1024) {
                            let path = file.0.clone();
                            let preview = tokio::task::spawn_blocking(move || {
                                fs::read(path)
                                    .ok()
                                    .and_then(|bytes| crate::pictures::decode(&bytes, true))
                            })
                            .await?;
                            self.pictures.insert(url.clone(), preview);
                        }
                        self.nav.files.insert(url, file);
                    }
                    self.put(Message {
                        attachments,
                        id: nonce.clone(),
                        channel_id: room.id.clone(),
                        author_id: me,
                        author: self.nav.local_profile.name.clone(),
                        content: c.body.clone(),
                        reply: c
                            .reply
                            .as_ref()
                            .and_then(|id| self.posts.iter().find(|m| m.id == *id))
                            .map(Reply::quoting),
                        editable: true,
                        ..Default::default()
                    });
                }
                self.samples.insert(room.id.clone(), self.posts.clone());
            } else {
                let service = self.service()?;
                if let Some(id) = &c.edit {
                    self.put(service.edit(&room.id, id, &c.body).await?);
                } else if let Some(note) = &c.note {
                    service.send_voice_note(&room.id, &me, note, &nonce).await?;
                } else {
                    let mut files = Vec::new();
                    for (name, file) in &c.files {
                        files.push((name.clone(), tokio::fs::read(&file.0).await?));
                    }
                    self.put(
                        service
                            .send(
                                &room.guild,
                                &room.id,
                                &c.body,
                                Sending {
                                    nonce: &nonce,
                                    enforce_nonce: true,
                                    reply: c.reply.as_deref(),
                                    ping_reply: false,
                                    silent: false,
                                },
                                files,
                            )
                            .await?,
                    );
                }
            }
            Ok(())
        }
        .await;
        self.sending = false;
        if let Err(e) = result {
            self.drafts.insert(room.id, c);
            return Err(e);
        }
        let restored = if c.edit.is_some() {
            c.before_edit
        } else {
            String::new()
        };
        self.drafts.insert(
            room.id,
            Composer {
                body: restored,
                ..Default::default()
            },
        );
        self.writing = true;
        self.collapse = true;
        self.notice.clear();
        self.tail += 1;
        self.remember_draft(None);
        self.saved.flush()?;
        Ok(())
    }
    async fn reaction(&mut self, id: String, key: String) -> Result<()> {
        ensure!(self.room.writable, "Read-only channel");
        let m = self
            .posts
            .iter()
            .find(|p| p.id == id)
            .context("Message no longer available")?;
        let add = !m.reactions.iter().any(|r| r.emoji == key && r.me);
        if self.preview {
            self.posts
                .iter_mut()
                .find(|p| p.id == id)
                .unwrap()
                .react(&key, &key, true, add);
            self.samples
                .insert(self.room.id.clone(), self.posts.clone());
        } else {
            self.service()?.react(&self.room.id, &id, &key, add).await?;
        }
        self.collapse = true;
        Ok(())
    }
    fn live(&mut self, e: Live) -> Result<bool> {
        self.detail_members_event(&e);
        if self.details.open() && matches!(&e, Live::Members { .. }) {
            self.draw();
        }
        self.tools.observe(&e);
        if self.tools.open() {
            self.sync_tools();
        }
        if let Live::Created(m) | Live::Updated(m) = &e {
            self.remember_message(m);
        }
        if let Live::Deleted { channel, id } = &e {
            if self
                .nav
                .recent
                .get(channel)
                .is_some_and(|(prev, _)| prev == id)
            {
                self.nav.recent.remove(channel);
            }
        }
        if let Live::Notify { message, .. } = &e {
            let item = Destination::new(
                &message.channel_id,
                &message.author,
                &view::initial(&message.author),
                &crate::spoiler::hide(&message.content),
                true,
            );
            self.nav.alerts.retain(|d| d.id != item.id);
            self.nav.alerts.insert(0, item);
            self.nav.alerts.truncate(80);
            self.platform.strings(
                "notifyMessage",
                &[
                    &message.channel_id,
                    &message.author,
                    &crate::spoiler::hide(&message.content),
                ],
            )?;
            return Ok(true);
        }
        match &e {
            Live::VoiceElsewhere { guild }
                if self.call.as_ref().is_some_and(|c| c.guild == *guild) =>
            {
                if let Some(mut c) = self.call.take() {
                    c.taken_over();
                }
                self.notice = "The call moved to another device.".into();
                return Ok(true);
            }
            Live::StreamEnded { stream_key, .. }
                if self
                    .call
                    .as_ref()
                    .is_some_and(|c| c.stream_key() == Some(stream_key)) =>
            {
                if let Some(c) = &mut self.call {
                    c.stop_share();
                }
                return Ok(true);
            }
            Live::VoiceChannel { guild, channel }
                if self
                    .call
                    .as_ref()
                    .is_some_and(|c| c.guild == *guild && c.channel != *channel) =>
            {
                self.call.take();
                self.notice = "Voice channel changed. Rejoin to continue.".into();
                return Ok(true);
            }
            Live::VoiceMoved { guild, .. }
                if self.call.as_ref().is_some_and(|c| c.guild == *guild) =>
            {
                self.call.take();
                self.notice = "Voice server changed. Rejoin to continue.".into();
                return Ok(true);
            }
            _ => {}
        }
        if self.page != Page::Chat {
            return Ok(false);
        }
        if self.apply_poll_vote(&e) {
            return Ok(true);
        }
        match e {
            Live::Created(m) if m.channel_id == self.room.id => {
                self.put(m);
                if self.can_ack_visible_room()
                    && let Some(service) = &self.discord
                {
                    service.mark_read(&self.room.guild, &self.room.id, true);
                }
            }
            Live::Updated(m) if m.channel_id == self.room.id => self.put(m),
            Live::Deleted { channel, id } if channel == self.room.id => {
                self.posts.retain(|m| m.id != id)
            }
            Live::Reaction {
                channel,
                message,
                key,
                emoji,
                me,
                added,
            } if channel == self.room.id => {
                if let Some(m) = self.posts.iter_mut().find(|m| m.id == message) {
                    m.react(&key, &emoji, me, added);
                }
            }
            Live::ReactionsCleared {
                channel,
                message,
                key,
            } if channel == self.room.id => {
                if let Some(m) = self.posts.iter_mut().find(|m| m.id == message) {
                    m.reactions
                        .retain(|r| key.as_ref().is_some_and(|k| k != &r.key));
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    async fn open_notification(&mut self) -> Result<bool> {
        if self.notification.is_empty() || self.snapshot.status != Status::Online {
            return Ok(false);
        }
        let id = std::mem::take(&mut self.notification);
        if self.snapshot.conversations.iter().any(|c| c.id == id) {
            self.page = Page::Inbox;
            self.section = 0;
        } else if let Some(g) = self.snapshot.servers.iter().find(|g| {
            g.text_channels()
                .chain(g.threads())
                .any(|c| c.id == id && c.marks.access != Access::Hidden && c.forum.is_none())
        }) {
            self.server = g.id.clone();
            self.page = Page::Channels;
        } else {
            return Ok(false);
        }
        self.choose(&id).await?;
        Ok(true)
    }
    async fn voice(&mut self, action: &str) -> Result<()> {
        self.notice.clear();
        match action {
            "join" => {
                ensure!(
                    self.call.is_none()
                        && !self.preview
                        && self.online()
                        && self.room.writable
                        && self.room.callable
                        && !self.room.request,
                    "Voice is unavailable in this conversation."
                );
                ensure!(
                    self.pending.is_none(),
                    "Finish the recording or attachment first."
                );
                let ring = self.room.guild == PRIVATE
                    && self
                        .snapshot
                        .conversations
                        .iter()
                        .any(|c| c.id == self.room.id && !c.call);
                self.notice = format!("Joining {}…", self.room.name);
                self.draw();
                self.sequence += 1;
                self.call = Some(
                    crate::call::Session::join(
                        self.service()?,
                        self.platform.clone(),
                        self.tx.clone(),
                        self.sequence,
                        self.room.guild.clone(),
                        self.room.id.clone(),
                        self.room.name.clone(),
                    )
                    .await?,
                );
                self.notice.clear();
                if ring && let Err(error) = self.service()?.ring(&self.room.id).await {
                    self.notice = format!("Connected. Could not ring the conversation: {error}");
                }
            }
            "leave" => {
                self.call.take();
            }
            "mute" => {
                if let Some(c) = &mut self.call {
                    if c.muted {
                        c.ask_mic()?;
                    } else {
                        c.mute(true)?;
                    }
                }
            }
            "share" => {
                if let Some(c) = &mut self.call {
                    if c.sharing.is_some() {
                        c.stop_share();
                    } else {
                        self.sequence += 1;
                        c.request_share(self.sequence)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn limit(&self) -> u64 {
        let tier = self.snapshot.account.as_ref().map_or(0, |a| a.premium_type);
        let guild = self
            .snapshot
            .servers
            .iter()
            .find(|g| g.id == self.room.guild)
            .map(|g| g.premium_tier);
        discord::api::upload_limit(tier, guild).min(32 * 1024 * 1024)
    }
    fn pick(&mut self, record: bool, media_only: bool) -> Result<()> {
        ensure!(
            self.room.writable && self.pending.is_none(),
            "Finish the current attachment first."
        );
        ensure!(
            !record || self.call.is_none(),
            "Leave the call before recording a voice message."
        );
        let c = self.compose();
        ensure!(
            c.edit.is_none() && c.note.is_none() && c.files.len() < discord::api::MAX_FILES,
            "Remove an attachment or finish editing first."
        );
        if record {
            ensure!(
                c.files.is_empty() && c.body.is_empty() && c.reply.is_none(),
                "Send or clear this draft before recording."
            );
        }
        self.sequence += 1;
        let id = self.sequence;
        if record {
            self.platform.record(id)?;
        } else {
            self.platform.pick(id, self.limit() as i64, media_only)?;
        }
        self.pending = Some((id, self.room.id.clone(), record));
        Ok(())
    }
    async fn picked(
        &mut self,
        id: i64,
        name: String,
        file: Option<StagedFile>,
        error: String,
    ) -> Result<()> {
        if self
            .pending
            .as_ref()
            .is_none_or(|(p, room, _)| *p != id || room != &self.room.id)
        {
            return Ok(());
        }
        let (_, _, record) = self.pending.take().unwrap();
        ensure!(error.is_empty(), "{error}");
        let Some(file) = file else { return Ok(()) };
        if record {
            let note = tokio::task::spawn_blocking(move || -> Result<voice_note::Note> {
                use std::io::Read;
                let mut source = fs::File::open(&file.0)?;
                let mut encoder = voice_note::Builder::new()?;
                let mut block = [0u8; 3840];
                loop {
                    let n = source.read(&mut block)?;
                    if n == 0 {
                        break;
                    }
                    ensure!(n % 2 == 0, "Incomplete audio frame");
                    let samples = block[..n]
                        .chunks_exact(2)
                        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                        .collect::<Vec<_>>();
                    if encoder.push(&samples)? {
                        break;
                    }
                }
                encoder.finish()
            })
            .await??;
            self.compose().note = Some(note);
        } else {
            let already: u64 = self
                .compose()
                .files
                .iter()
                .map(|(_, f)| fs::metadata(&f.0).map_or(0, |m| m.len()))
                .sum();
            ensure!(
                already + fs::metadata(&file.0)?.len() <= 32 * 1024 * 1024,
                "Attachments exceed the 32 MiB mobile staging limit."
            );
            self.compose().files.push((safe_name(&name), file));
        }
        self.compose().nonce = None;
        self.notice.clear();
        Ok(())
    }
    async fn media(&mut self, id: &str, index: i32) -> Result<()> {
        let file = self
            .posts
            .iter()
            .find(|p| p.id == id)
            .and_then(|m| m.attachments.get(index as usize))
            .context("Attachment no longer available")?;
        if file.spoiler
            && self
                .revealed
                .entry(id.into())
                .or_default()
                .files
                .insert(index as usize)
        {
            return Ok(());
        }
        let name = file.name.clone();
        let url = file
            .video_url
            .as_deref()
            .or(file.view_url.as_deref())
            .unwrap_or(&file.url)
            .to_owned();
        ensure!(
            !discord::ready::lottie_sticker(&url),
            "This sticker uses Lottie animation, which the Android viewer cannot play yet."
        );
        if self.preview {
            if let Some((name, data)) = preview::attachment(&url) {
                return self.show_media(name, data);
            }
            let file = self
                .nav
                .files
                .get(&url)
                .context("Local attachment expired")?;
            return self.show_media(&name, &fs::read(&file.0)?);
        }
        let bytes = self.service()?.download_now(&url, 32 * 1024 * 1024).await?;
        self.show_media(&name, &bytes)
    }
    fn show_media(&self, name: &str, data: &[u8]) -> Result<()> {
        let folder = self.platform.cache.join("media").join(message::nonce());
        fs::create_dir_all(&folder)?;
        let path = folder.join(safe_name(name));
        fs::write(&path, data)?;
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let mime = if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
            "image/gif"
        } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
            "image/webp"
        } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
            "image/jpeg"
        } else if data.starts_with(b"OggS") {
            "audio/ogg"
        } else if data.get(4..8) == Some(b"ftyp") {
            if extension == "m4a" {
                "audio/mp4"
            } else {
                "video/mp4"
            }
        } else if data.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
            if extension == "weba" {
                "audio/webm"
            } else {
                "video/webm"
            }
        } else {
            match extension.as_str() {
                "ogg" | "opus" => "audio/ogg",
                "mp3" => "audio/mpeg",
                "m4a" | "aac" | "wav" => "audio/*",
                "mp4" | "webm" | "mkv" | "mov" => "video/*",
                "png" | "jpeg" | "jpg" | "gif" | "webp" | "bmp" => "image/*",
                "pdf" => "application/pdf",
                "txt" => "text/plain",
                _ => "application/octet-stream",
            }
        };
        self.platform.strings(
            "openMedia",
            &[path.to_str().context("Invalid media path")?, mime],
        )
    }
}
fn safe_name(name: &str) -> String {
    let safe: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let limit = safe
        .char_indices()
        .find(|(i, _)| *i > 200)
        .map_or(safe.len(), |(i, _)| i);
    let safe = safe[..limit].to_string();
    if safe.trim_matches(['.', ' ']).is_empty() {
        "attachment".into()
    } else {
        safe
    }
}
