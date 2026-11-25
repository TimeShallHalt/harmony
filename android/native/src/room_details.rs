use super::{Command, Page, Room, Sender, Shell};
use crate::preview;
use crate::runtime::BackgroundTask;
use anyhow::{Context, Result, ensure};
use std::{collections::HashMap, sync::Arc, time::Duration};
use yapfast_discord::{
    api::{Span, THREAD_PAGE},
    member_list::{ListedMember, Presence},
    profile::Profile,
    ready::{Access, Channel, PRIVATE},
    service::{Live, Snapshot, ThreadPage},
};

pub(crate) enum Event {
    Input(String, String, String),
    Threads(u64, Result<ThreadPage, String>),
    Profile(u64, Result<Profile, String>),
    Waiting(u64),
}

#[derive(Clone, Default)]
struct Item {
    id: String,
    name: String,
    caption: String,
    body: String,
    group: String,
    extra: String,
    avatar: String,
    status: &'static str,
    action: &'static str,
}

impl Item {
    fn person(id: impl Into<String>, name: impl Into<String>, avatar: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            avatar: avatar.into(),
            ..Default::default()
        }
    }

    fn listed(member: &ListedMember) -> Self {
        let status = status(member.presence);
        Self {
            id: member.user_id.clone(),
            name: member.name.clone(),
            avatar: member.avatar.clone(),
            caption: member
                .activity
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(|| member.username.as_ref().map(|u| format!("@{u}")))
                .unwrap_or_else(|| status.into()),
            extra: if member.bot {
                "APP".into()
            } else {
                String::new()
            },
            group: member.group.clone(),
            status,
            ..Default::default()
        }
    }

    fn model(self, image: Option<Arc<crate::pictures::Thumbnail>>) -> crate::DetailRow {
        crate::DetailRow {
            id: self.id.into(),
            initial: crate::view::initial(&self.name).into(),
            name: self.name.into(),
            caption: self.caption.into(),
            body: self.body.into(),
            section: self.group.into(),
            extra: self.extra.into(),
            status: self.status.into(),
            action: self.action.into(),
            avatar: preview::picture(&self.avatar)
                .or_else(|| image.map(|i| i.image()))
                .unwrap_or_default(),
        }
    }
}

fn status(value: Presence) -> &'static str {
    match value.base() {
        Presence::Online => "Online",
        Presence::Mobile => "Online on mobile",
        Presence::Idle => "Idle",
        Presence::DoNotDisturb => "Do not disturb",
        Presence::Offline => "Offline",
        Presence::Clients(_) => "",
    }
}

#[derive(Clone)]
struct ReturnRoom {
    source: Room,
    target: String,
    mode: &'static str,
    tab: &'static str,
    query: String,
}

const PREVIEW_THREADS: &[(&str, &str, bool, &str)] = &[
    (
        "afterlight-shots",
        "Afterlight screenshots",
        false,
        "Drop your favorite station shots here. I’ll add them to the contact sheet.",
    ),
    (
        "afterlight-mix",
        "Station sound mix",
        true,
        "The final mix is ready. Thanks for tracking down the tape hiss!",
    ),
];

#[derive(Default)]
pub(super) struct Details {
    can_threads: bool,
    return_room: Option<ReturnRoom>,
    forget_list: bool,
    preview_joins: HashMap<String, bool>,
    mode: &'static str,
    tab: &'static str,
    source: String,
    guild: String,
    title: String,
    epoch: u64,
    generation: u64,
    busy: bool,
    dirty: bool,
    note: String,
    query: String,
    roster: Option<(u64, String, Vec<Item>)>,
    chosen: Option<Item>,
    profile: Option<Profile>,
    remote: Vec<Channel>,
    more: bool,
    offset: u32,
    membership: HashMap<String, bool>,
    job: Option<BackgroundTask<()>>,
    opened_thread: String,
}

impl Details {
    fn cancel(&mut self) {
        self.job.take();
        self.generation = self.generation.wrapping_add(1);
        self.busy = false;
    }

    pub(super) fn open(&self) -> bool {
        !self.mode.is_empty()
    }

    pub(super) fn close(&mut self) {
        self.cancel();
        self.forget_list |= self.mode == "threads";
        self.mode = "";
        self.tab = "";
        self.query.clear();
        self.note.clear();
        self.remote.clear();
        self.chosen = None;
        self.profile = None;
        self.more = false;
        self.membership.clear();
        self.dirty = true;
    }

    pub(super) fn invalidate(&mut self) {
        self.dirty |= self.open();
    }

    pub(super) fn back(&mut self) {
        if self.mode == "profile" {
            self.cancel();
            self.chosen = None;
            self.profile = None;
            self.mode = "members";
            self.note.clear();
            self.dirty = true;
        } else {
            self.close();
        }
    }

    pub(super) fn avatars(&self) -> impl Iterator<Item = String> + '_ {
        let members = self
            .roster
            .iter()
            .filter(|_| self.mode == "members")
            .flat_map(|(_, _, rows)| rows.iter().take(80).map(|p| p.avatar.clone()));
        let card = self.profile.iter().map(|p| p.avatar.clone());
        members.chain(card)
    }
}

pub(super) fn bind(window: &crate::MobileWindow, tx: Sender) {
    window.on_detail_command(move |action, id, extra| {
        let _ = tx.send(Command::Details(Event::Input(
            action.into(),
            id.into(),
            extra.into(),
        )));
    });
}

impl Shell {
    // Discord sends the parent channel's readers while a thread is open.
    fn details_parent(&self) -> Option<(String, String)> {
        if self.room.guild == PRIVATE {
            return None;
        }
        if self.preview {
            if PREVIEW_THREADS
                .iter()
                .any(|(id, _, _, _)| *id == self.room.id)
            {
                return Some((preview::FEATURED_CHANNEL.into(), "general".into()));
            }
            return Some((self.room.id.clone(), self.room.name.clone()));
        }
        let server = self
            .snapshot
            .servers
            .iter()
            .find(|s| s.id == self.room.guild)?;
        let room = server.channel(&self.room.id)?;
        if room.marks.access == Access::Hidden {
            return None;
        }
        let parent = if room.thread.is_some() {
            server.channel(room.parent.as_deref()?)?
        } else {
            room
        };
        (parent.marks.access != Access::Hidden).then(|| (parent.id.clone(), parent.name.clone()))
    }

    pub(super) fn detail_threads_available(&self) -> bool {
        if self.preview {
            return self.room.guild != PRIVATE;
        }
        let Some((id, _)) = self.details_parent() else {
            return false;
        };
        self.snapshot
            .servers
            .iter()
            .find(|s| s.id == self.room.guild)
            .is_some_and(|s| s.text_channels().any(|channel| channel.id == id))
    }

    pub(super) fn detail_members_event(&mut self, event: &Live) {
        let Live::Members {
            channel, members, ..
        } = event
        else {
            return;
        };
        if self.page != Page::Chat {
            return;
        }
        let expected = self
            .details_parent()
            .map(|(id, _)| id)
            .unwrap_or_else(|| self.room.id.clone());
        if *channel != expected {
            return;
        }
        self.details.roster = Some((
            self.epoch,
            channel.clone(),
            members.iter().map(Item::listed).collect(),
        ));
        if self.details.mode == "members" {
            self.details.cancel();
            self.details.note.clear();
        }
        self.details.invalidate();
    }

    // The service publishes a newly opened archive into snapshots asynchronously.
    // A queued older snapshot must not kick the reader back out of that thread.
    pub(super) fn details_snapshot(&self, incoming: Arc<Snapshot>) -> Arc<Snapshot> {
        if !self.details.opened_thread.is_empty()
            && self.details.opened_thread == self.room.id
            && let Some(service) = &self.discord
        {
            return service.snapshots().borrow().clone();
        }
        incoming
    }

    fn detail_roster(&self) -> Option<Vec<Item>> {
        if self.preview {
            let mut members = vec![Item::person(
                self.me(),
                preview::LOCAL_NAME,
                preview::avatar(self.me()),
            )];
            if let Some(peers) = self.create.members.get(&self.room.id) {
                members.extend(
                    peers
                        .iter()
                        .map(|(id, name)| Item::person(id, name, preview::avatar(id))),
                );
            }
            if self.room.guild == PRIVATE
                && self.room.id != "co-op"
                && let Some(peer) = preview::room_name(&self.room.id)
            {
                members.push(Item::person(
                    &self.room.id,
                    peer,
                    preview::avatar(&self.room.id),
                ));
            }
            for message in &self.posts {
                if !message.author_id.is_empty()
                    && !members.iter().any(|m| m.id == message.author_id)
                {
                    members.push(Item::person(
                        &message.author_id,
                        &message.author,
                        preview::avatar(&message.author_id),
                    ));
                }
            }
            return Some(members);
        }
        if self.room.guild == PRIVATE {
            let dm = self
                .snapshot
                .conversations
                .iter()
                .find(|c| c.id == self.room.id)?;
            if let Some(person) = &dm.person {
                let mut other =
                    Item::person(person, &dm.name, dm.avatar.clone().unwrap_or_default());
                other.status = dm.presence.map(status).unwrap_or("");
                other.caption = dm.activity.clone().unwrap_or_else(|| other.status.into());
                let mut members = vec![other];
                if let Some(account) = &self.snapshot.account {
                    let mut own = Item::person(self.me(), &account.name, &account.avatar);
                    own.caption = format!("@{}", account.username);
                    members.push(own);
                }
                return Some(members);
            }
        }
        let channel = self
            .details_parent()
            .map(|(id, _)| id)
            .unwrap_or_else(|| self.room.id.clone());
        self.details
            .roster
            .as_ref()
            .filter(|(epoch, id, _)| *epoch == self.epoch && *id == channel)
            .map(|(_, _, members)| members.clone())
    }

    pub(super) async fn details_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Input(action, id, extra) => {
                if self.page != Page::Chat {
                    return Ok(());
                }
                if let Err(error) = self.details_input(&action, &id, &extra).await {
                    if !self.details.open() {
                        return Err(error);
                    }
                    self.details.busy = false;
                    self.details.note = error.to_string();
                }
            }
            Event::Threads(generation, result)
                if self.details.generation == generation && self.details.mode == "threads" =>
            {
                self.details.job.take();
                self.details.busy = false;
                match result {
                    Ok(page) => {
                        self.details.more = page.more;
                        self.details.offset = self.details.offset.saturating_add(THREAD_PAGE);
                        for channel in page.threads {
                            if !self.details.remote.iter().any(|c| c.id == channel.id) {
                                self.details.remote.push(channel);
                            }
                        }
                        self.details.note.clear();
                    }
                    Err(error) => self.details.note = error,
                }
            }
            Event::Profile(generation, result)
                if self.details.generation == generation && self.details.mode == "profile" =>
            {
                self.details.job.take();
                self.details.busy = false;
                match result {
                    Ok(p) => {
                        self.details.profile = Some(p);
                        self.details.note.clear();
                    }
                    Err(error) => self.details.note = error,
                }
            }
            Event::Waiting(generation)
                if self.details.generation == generation && self.details.mode == "members" =>
            {
                self.details.job.take();
                self.details.busy = false;
                self.details.note = "The member list hasn’t arrived. Tap to retry.".into();
            }
            _ => return Ok(()),
        }
        self.details.dirty = true;
        Ok(())
    }

    async fn details_input(&mut self, action: &str, id: &str, _extra: &str) -> Result<()> {
        if matches!(action, "members" | "threads") {
            if action == "threads" {
                ensure!(
                    self.detail_threads_available(),
                    "There are no threads in this conversation."
                );
            } else if self.room.guild != PRIVATE {
                ensure!(
                    self.details_parent().is_some(),
                    "You no longer have access to this channel."
                );
            }
            self.tools.close();
            self.details.close();
            self.details.source = self.room.id.clone();
            self.details.guild = self.room.guild.clone();
            self.details.epoch = self.epoch;
            self.details.title = self
                .details_parent()
                .map(|(_, name)| name)
                .unwrap_or_else(|| self.room.name.clone());
            self.details.mode = if action == "threads" {
                "threads"
            } else {
                "members"
            };
            self.details.tab = "active";
            if self.details.mode == "members" {
                self.request_detail_roster()?;
            }
            return Ok(());
        }
        if !self.details.open()
            || self.details.source != self.room.id
            || self.details.epoch != self.epoch
        {
            return Ok(());
        }
        match action {
            "back" | "close" => self.details.back(),
            "retry" if self.details.mode == "members" => self.request_detail_roster()?,
            "filter" if self.details.mode == "members" => self.details.query = id.to_owned(),
            "profile" if self.details.mode == "members" => self.request_detail_profile(id)?,
            "retry" if self.details.mode == "profile" => {
                if let Some(user) = self.details.chosen.as_ref().map(|p| p.id.clone()) {
                    self.request_detail_profile(&user)?;
                }
            }
            "message" if self.details.mode == "profile" => self.details_message().await?,
            "tab" if self.details.mode == "threads" && matches!(id, "active" | "archived") => {
                self.details.cancel();
                self.details.tab = if id == "active" { "active" } else { "archived" };
                self.details.query.clear();
                self.reset_detail_threads();
                if self.details.tab == "archived" {
                    self.request_detail_threads()?;
                }
            }
            "search" if self.details.mode == "threads" => {
                self.details.cancel();
                self.details.query = id.trim().to_string();
                self.details.tab = if self.details.query.is_empty() {
                    "active"
                } else {
                    "search"
                };
                self.reset_detail_threads();
                if self.details.tab == "search" {
                    self.request_detail_threads()?;
                }
            }
            "more" if self.details.mode == "threads" && self.details.more && !self.details.busy => {
                self.request_detail_threads()?
            }
            "retry"
                if self.details.mode == "threads"
                    && !self.details.busy
                    && self.details.tab != "active" =>
            {
                self.request_detail_threads()?
            }
            "open" if self.details.mode == "threads" => self.details_open_thread(id).await?,
            "join" | "leave" if self.details.mode == "threads" => {
                let (channel, access) = self
                    .detail_threads()
                    .into_iter()
                    .find(|(c, _)| c.id == id)
                    .context("This thread is unavailable.")?;
                let thread = channel.thread.as_ref().context("This is not a thread.")?;
                let join = action == "join";
                ensure!(
                    !thread.archived
                        && (thread.joined || access == Access::Join)
                        && thread.joined != join,
                    "Thread membership has changed."
                );
                if self.preview {
                    self.details.preview_joins.insert(id.into(), join);
                } else {
                    self.service()?
                        .join_thread(id, join, "Thread Browser")
                        .await?;
                }
                self.details.membership.insert(id.into(), join);
                self.details.note.clear();
            }
            _ => {}
        }
        Ok(())
    }

    fn request_detail_roster(&mut self) -> Result<()> {
        self.details.cancel();
        self.details.note.clear();
        if self.preview {
            return Ok(());
        }
        let service = self.service()?;
        service.view(&self.room.guild, Some(&self.room.id));
        if self.detail_roster().is_none() {
            self.details.busy = true;
            let generation = self.details.generation;
            let tx = self.tx.clone();
            self.details.job = Some(self.runtime.spawn(async move {
                tokio::time::sleep(Duration::from_secs(8)).await;
                let _ = tx.send(Command::Details(Event::Waiting(generation)));
            }));
        }
        Ok(())
    }

    fn request_detail_profile(&mut self, id: &str) -> Result<()> {
        let member = self
            .detail_roster()
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.id == id)
            .context("This member is no longer listed.")?;
        self.details.cancel();
        self.details.chosen = Some(member);
        self.details.profile = None;
        self.details.mode = "profile";
        self.details.note.clear();
        if self.preview {
            return Ok(());
        }
        let service = self.service()?;
        self.details.busy = true;
        let generation = self.details.generation;
        let guild = self.room.guild.clone();
        let user = id.to_string();
        let tx = self.tx.clone();
        self.details.job = Some(self.runtime.spawn(async move {
            let result = service.profile(&guild, &user, |_| {}).await;
            let _ = tx.send(Command::Details(Event::Profile(
                generation,
                result.map_err(|e| e.to_string()),
            )));
        }));
        Ok(())
    }

    fn reset_detail_threads(&mut self) {
        self.details.remote.clear();
        self.details.more = false;
        self.details.offset = 0;
        self.details.note.clear();
    }

    fn request_detail_threads(&mut self) -> Result<()> {
        if self.preview {
            return Ok(());
        }
        let (parent, _) = self
            .details_parent()
            .context("This channel is unavailable.")?;
        let service = self.service()?;
        self.details.cancel();
        self.details.busy = true;
        self.details.note.clear();
        let guild = self.room.guild.clone();
        let query = (!self.details.query.is_empty()).then(|| self.details.query.clone());
        let offset = self.details.offset;
        let generation = self.details.generation;
        let tx = self.tx.clone();
        self.details.job = Some(self.runtime.spawn(async move {
            let result = service
                .threads(&guild, &parent, query.as_deref(), offset)
                .await;
            let _ = tx.send(Command::Details(Event::Threads(
                generation,
                result.map_err(|e| e.to_string()),
            )));
        }));
        Ok(())
    }

    fn detail_threads(&self) -> Vec<(Channel, Access)> {
        let Some((parent, _)) = self.details_parent() else {
            return vec![];
        };
        if self.preview {
            if parent != preview::FEATURED_CHANNEL {
                return vec![];
            }
            return PREVIEW_THREADS
                .iter()
                .filter(|(_, name, archived, _)| match self.details.tab {
                    "archived" => *archived,
                    "search" => name
                        .to_lowercase()
                        .contains(&self.details.query.to_lowercase()),
                    _ => !*archived,
                })
                .map(|(id, name, archived, _)| {
                    let joined = self
                        .details
                        .preview_joins
                        .get(*id)
                        .copied()
                        .unwrap_or(*archived);
                    let count = self.samples.get(*id).map_or(1, Vec::len) as u64;
                    (
                        Channel {
                            id: (*id).into(),
                            name: (*name).into(),
                            parent: Some(parent.clone()),
                            thread: Some(Box::new(yapfast_discord::ready::Thread {
                                owner: Some("mira".into()),
                                joined,
                                archived: *archived,
                                members: if joined { 3 } else { 2 },
                                messages: count,
                                ..Default::default()
                            })),
                            ..Default::default()
                        },
                        Access::Join,
                    )
                })
                .collect();
        }
        let Some(server) = self
            .snapshot
            .servers
            .iter()
            .find(|s| s.id == self.room.guild)
        else {
            return vec![];
        };
        let active = self.details.tab == "active";
        let mut found: Vec<_> = if active {
            server
                .threads()
                .filter(|c| c.marks.access != Access::Hidden)
                .map(|c| c.channel.clone())
                .collect()
        } else {
            self.details.remote.clone()
        };
        if active {
            found.sort_by_key(|c| {
                std::cmp::Reverse(c.thread.as_ref().map_or(0, |t| t.last_message))
            });
        }
        found
            .into_iter()
            .filter_map(|mut channel| {
                if channel.parent.as_deref() != Some(parent.as_str()) {
                    return None;
                }
                // Prefer the up-to-date gateway version over a REST search entry.
                if let Some(known) = server.channel(&channel.id) {
                    channel = known.channel.clone();
                }
                let detail = channel.thread.as_deref_mut()?;
                if active && detail.archived || self.details.tab == "archived" && !detail.archived {
                    return None;
                }
                if let Some(joined) = self.details.membership.get(&channel.id) {
                    detail.joined = *joined;
                }
                let access = server.access(&channel, self.me(), &server.marks.roles);
                (access != Access::Hidden).then_some((channel, access))
            })
            .collect()
    }

    async fn details_open_thread(&mut self, id: &str) -> Result<()> {
        let (channel, access) = self
            .detail_threads()
            .into_iter()
            .find(|(c, _)| c.id == id)
            .context("This thread is no longer available.")?;
        if self.preview {
            let (_, _, _, body) = PREVIEW_THREADS
                .iter()
                .find(|(thread, _, _, _)| *thread == id)
                .context("This preview thread is unavailable.")?;
            self.samples.entry(id.into()).or_insert_with(|| {
                vec![yapfast_discord::message::Message {
                    id: "1500000000000002100".into(),
                    author: "Mira Vale".into(),
                    author_id: "mira".into(),
                    channel_id: id.into(),
                    content: (*body).into(),
                    editable: true,
                    ..Default::default()
                }]
            });
            self.remember_detail_return(id);
            self.enter_detail_preview(Room {
                guild: self.room.guild.clone(),
                id: id.into(),
                name: channel.name,
                writable: true,
                ..Default::default()
            });
            return Ok(());
        }
        let service = self.service()?;
        let guild = self.room.guild.clone();
        // Fetch before changing the visible room, so failed navigation leaves the list usable.
        let history = service.history(&guild, id, Span::Latest).await?;
        let mut snapshots = service.snapshots();
        service
            .open_thread(&guild, id, service.listed_thread(id), false)
            .await?;
        drop(
            tokio::time::timeout(
                Duration::from_secs(5),
                snapshots.wait_for(|s| {
                    s.servers
                        .iter()
                        .find(|g| g.id == guild)
                        .is_some_and(|g| g.channel(id).is_some())
                }),
            )
            .await
            .context("Discord did not open this thread in time.")?
            .context("The Discord connection closed while opening this thread.")?,
        );
        self.snapshot = snapshots.borrow().clone();
        self.remember_detail_return(id);
        self.details.opened_thread = id.into();
        self.enter_detailed_room(
            Room {
                guild,
                id: id.into(),
                name: channel.name,
                writable: access == Access::Join,
                ..Default::default()
            },
            history,
        );
        Ok(())
    }

    fn can_message_member(&self, id: &str) -> bool {
        !id.is_empty()
            && id != self.me()
            && !self.room.request
            && !self.snapshot.blocked.iter().any(|b| b == id)
            && !self.snapshot.ignored.iter().any(|b| b == id)
    }

    async fn details_message(&mut self) -> Result<()> {
        let peer = self
            .details
            .chosen
            .clone()
            .context("This member is unavailable.")?;
        ensure!(
            self.can_message_member(&peer.id),
            "You cannot message this member."
        );
        if self.preview {
            // Reuse the contact's preview DM so its draft/history survive switching.
            let mapped = if peer.id == "sample" {
                "alex".to_string()
            } else {
                peer.id
            };
            self.remember_detail_return(&mapped);
            self.enter_detail_preview(Room {
                guild: PRIVATE.into(),
                id: mapped,
                name: peer.name,
                writable: true,
                ..Default::default()
            });
            return Ok(());
        }
        let service = self.service()?;
        let id = service.open_dm(&peer.id).await?;
        let history = service.history(PRIVATE, &id, Span::Latest).await?;
        self.remember_detail_return(&id);
        self.details.opened_thread.clear();
        self.enter_detailed_room(
            Room {
                guild: PRIVATE.into(),
                id,
                name: peer.name,
                writable: true,
                ..Default::default()
            },
            history,
        );
        Ok(())
    }

    fn remember_detail_return(&mut self, target: &str) {
        if self.room.id != target {
            if self.preview {
                self.samples
                    .insert(self.room.id.clone(), self.posts.clone());
            }
            self.details.return_room = Some(ReturnRoom {
                source: self.room.clone(),
                target: target.into(),
                mode: if self.details.mode == "profile" {
                    "members"
                } else {
                    "threads"
                },
                tab: self.details.tab,
                query: self.details.query.clone(),
            });
        }
    }

    pub(super) fn has_detail_return(&self) -> bool {
        self.page == Page::Chat
            && self
                .details
                .return_room
                .as_ref()
                .is_some_and(|r| r.target == self.room.id)
    }

    pub(super) async fn return_from_detail(&mut self) -> Result<()> {
        let back = self
            .details
            .return_room
            .clone()
            .context("The previous conversation is unavailable.")?;
        if self.preview {
            self.samples
                .insert(self.room.id.clone(), self.posts.clone());
            self.enter_detail_preview(back.source);
        } else {
            let service = self.service()?;
            let history = service
                .history(&back.source.guild, &back.source.id, Span::Latest)
                .await?;
            self.enter_detailed_room(back.source, history);
        }
        self.details.return_room.take();
        self.details_input(back.mode, "", "").await?;
        if back.mode == "members" {
            self.details.query = back.query;
        } else if back.tab == "archived" {
            self.details_input("tab", "archived", "").await?;
        } else if back.tab == "search" {
            self.details_input("search", &back.query, "").await?;
        }
        self.details.dirty = true;
        Ok(())
    }

    fn enter_detail_preview(&mut self, room: Room) {
        self.cancel_pick();
        self.details.close();
        self.room = room;
        self.posts = self
            .samples
            .entry(self.room.id.clone())
            .or_insert_with(|| preview::messages(&self.room.id))
            .clone();
        self.oldest = None;
        self.older = false;
        self.writing = true;
        self.collapse = true;
        self.tail += 1;
        self.notice.clear();
    }

    fn enter_detailed_room(&mut self, room: Room, mut history: yapfast_discord::message::Page) {
        self.cancel_pick();
        self.details.close();
        self.room = room;
        self.update_room();
        self.posts = std::mem::take(&mut history.messages);
        self.posts
            .sort_by_key(yapfast_discord::message::Message::order);
        self.oldest = history.oldest;
        self.older = history.full && history.oldest.is_some();
        self.writing = true;
        self.collapse = true;
        self.tail += 1;
        self.notice.clear();
        if let Some(service) = &self.discord {
            service.view(&self.room.guild, Some(&self.room.id));
            if self.foreground {
                service.mark_read(&self.room.guild, &self.room.id, true);
            }
        }
    }

    pub(super) fn sync_details(&mut self) {
        let changed_room = self.details.source != self.room.id
            || self.details.epoch != self.epoch
            || self.page != Page::Chat;
        if !self.has_detail_return() {
            self.details.return_room.take();
        }
        let can_threads =
            matches!(self.page, Page::Chat | Page::Details) && self.detail_threads_available();
        if can_threads != self.details.can_threads {
            self.details.can_threads = can_threads;
            self.details.dirty = true;
        }
        if changed_room && self.details.open() {
            self.details.close();
        }
        if self.page != Page::Chat
            || self
                .details
                .roster
                .as_ref()
                .is_some_and(|(epoch, _, _)| *epoch != self.epoch)
        {
            self.details.roster = None;
        }
        if self.details.opened_thread != self.room.id {
            self.details.opened_thread.clear();
        }
        if std::mem::take(&mut self.details.forget_list)
            && let Some(d) = &self.discord
        {
            d.forget_listed_threads();
        }
        if !std::mem::take(&mut self.details.dirty) {
            return;
        }
        let mode = self.details.mode;
        let tab = self.details.tab;
        let mut items = Vec::new();
        let mut card = self.details.chosen.clone().unwrap_or_default();
        let mut note = self.details.note.clone();
        if mode == "members" {
            let query = self.details.query.to_lowercase();
            let mut section = String::new();
            let members = self.detail_roster().unwrap_or_default();
            for mut member in members.into_iter().filter(|m| {
                query.is_empty()
                    || m.name.to_lowercase().contains(&query)
                    || m.caption.to_lowercase().contains(&query)
            }) {
                if member.group == section {
                    member.group.clear();
                } else {
                    section = member.group.clone();
                }
                items.push(member);
            }
        } else if mode == "profile" {
            card.action = if self.can_message_member(&card.id) {
                "message"
            } else {
                ""
            };
            if let Some(p) = self.details.profile.as_ref() {
                card.name = p.name.clone();
                card.caption = [Some(format!("@{}", p.username)), p.pronouns.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · ");
                card.avatar = p.avatar.clone();
                card.body = p
                    .bio
                    .as_deref()
                    .map(crate::spoiler::hide)
                    .unwrap_or_default();
                card.extra = self
                    .snapshot
                    .servers
                    .iter()
                    .find(|s| s.id == self.room.guild)
                    .map(|s| {
                        p.roles
                            .iter()
                            .filter(|id| **id != s.id)
                            .filter_map(|id| s.role_names.get(id).cloned())
                            .collect::<Vec<_>>()
                            .join("  ·  ")
                    })
                    .unwrap_or_default();
            }
        } else if mode == "threads" {
            for (channel, access) in self.detail_threads() {
                let Some(t) = channel.thread.as_ref() else {
                    continue;
                };
                let kind = if t.private {
                    "Private thread"
                } else {
                    "Thread"
                };
                let other = if t.locked {
                    "Locked"
                } else if t.archived {
                    "Archived"
                } else if t.joined {
                    "Joined"
                } else {
                    ""
                };
                let mut item = Item {
                    id: channel.id,
                    name: channel.name,
                    caption: format!(
                        "{} {} · {} {}",
                        t.members,
                        if t.members == 1 { "member" } else { "members" },
                        t.messages,
                        if t.messages == 1 {
                            "message"
                        } else {
                            "messages"
                        }
                    ),
                    extra: [kind, other]
                        .into_iter()
                        .filter(|v| !v.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · "),
                    action: if t.archived {
                        ""
                    } else if t.joined {
                        "leave"
                    } else if access == Access::Join {
                        "join"
                    } else {
                        ""
                    },
                    ..Default::default()
                };
                if let Some(recent) = &t.recent
                    && !self.snapshot.blocked.contains(&recent.author)
                    && !self.snapshot.ignored.contains(&recent.author)
                {
                    item.body = crate::spoiler::hide(&recent.content)
                        .chars()
                        .take(140)
                        .collect();
                }
                items.push(item);
            }
        }
        if note.is_empty() && mode == "members" && !self.details.busy && items.is_empty() {
            note = if self.details.query.is_empty() {
                "No members to show."
            } else {
                "No matching members."
            }
            .into();
        }
        let busy = self.details.busy;
        let more = self.details.more;
        let retry = !self.details.note.is_empty() && !busy;
        let title = self.details.title.clone();
        let is_thread = self.details.source
            != self
                .details_parent()
                .map(|(id, _)| id)
                .unwrap_or_else(|| self.room.id.clone());
        let pictures = items
            .into_iter()
            .map(|item| {
                let image = self.pictures.get(&item.avatar).and_then(Option::clone);
                (item, image)
            })
            .collect::<Vec<_>>();
        let card_image = self.pictures.get(&card.avatar).and_then(Option::clone);
        let _ = self.ui.upgrade_in_event_loop(move |w| {
            w.set_detail_can_threads(can_threads);
            w.set_detail_mode(mode.into());
            w.set_detail_tab(tab.into());
            w.set_detail_title(title.into());
            w.set_detail_note(note.into());
            w.set_detail_busy(busy);
            w.set_detail_more(more);
            w.set_detail_retry(retry);
            w.set_detail_parent(is_thread);
            w.set_detail_card(card.model(card_image));
            let rows = pictures
                .into_iter()
                .map(|(item, img)| item.model(img))
                .collect::<Vec<_>>();
            w.set_detail_rows(std::rc::Rc::new(slint::VecModel::from(rows)).into());
        });
    }
}
