use super::{Command, Page, Room, Sender, Shell};
use crate::runtime::BackgroundTask;
use anyhow::{Context, Result, ensure};
use std::collections::{HashMap, HashSet};
use yapfast_discord::{
    api::Span,
    message::Message,
    ready::{Access, Deletes, PRIVATE, permission},
    search::Query,
    service::Found,
};

pub(crate) enum Event {
    Input(String, String, String),
    Loaded(u64, Result<Found, String>),
}

#[derive(Default)]
pub(super) struct Tools {
    mode: &'static str,
    source: String,
    epoch: u64,
    query: String,
    whole_server: bool,
    offset: u32,
    total: u32,
    more: bool,
    hits: Vec<Message>,
    note: String,
    busy: bool,
    dirty: bool,
    generation: u64,
    job: Option<BackgroundTask<()>>,
    jump: String,
    pub(super) past: bool,
}

impl Tools {
    pub(super) fn close(&mut self) {
        self.job.take();
        self.generation = self.generation.wrapping_add(1);
        self.mode = "";
        self.query.clear();
        self.hits.clear();
        self.note.clear();
        self.busy = false;
        self.more = false;
        self.offset = 0;
        self.total = 0;
        self.dirty = true;
    }
    pub(super) fn open(&self) -> bool {
        !self.mode.is_empty()
    }
    pub(super) fn invalidate(&mut self) {
        self.dirty |= self.open();
    }

    pub(super) fn observe(&mut self, event: &yapfast_discord::service::Live) {
        use yapfast_discord::service::Live;
        if !self.open() {
            return;
        }
        match event {
            Live::Updated(message) | Live::Created(message) => {
                if let Some(index) = self
                    .hits
                    .iter()
                    .position(|m| m.id == message.id && m.channel_id == message.channel_id)
                {
                    if self.mode == "pins" && !message.pinned {
                        self.hits.remove(index);
                    } else {
                        self.hits[index] = message.clone();
                    }
                    self.dirty = true;
                } else if self.mode == "pins" && message.channel_id == self.source && message.pinned
                {
                    self.hits.insert(0, message.clone());
                    self.dirty = true;
                }
            }
            Live::Deleted { channel, id } => {
                let len = self.hits.len();
                self.hits
                    .retain(|m| m.channel_id != *channel || m.id != *id);
                self.dirty |= len != self.hits.len();
            }
            _ => {}
        }
    }
}

pub(super) fn bind(window: &crate::MobileWindow, tx: Sender) {
    window.on_tool_command(move |action, text, scope| {
        let _ = tx.send(Command::Tools(Event::Input(
            action.into(),
            text.into(),
            scope.into(),
        )));
    });
}

// Match the desktop message-menu permission calculations. Validate again before every mutation.
impl Shell {
    pub(super) fn can_ack_visible_room(&self) -> bool {
        self.page == Page::Chat
            && self.foreground
            && !self.tools.past
            && !self.tools.open()
            && !self.details.open()
            && !self.create.open()
            && !self.expressions.open()
    }

    pub(super) fn preview_read_state(&mut self, id: &str, read: bool) {
        let read_key = format!("read/{id}");
        let unread_key = format!("unread/{id}");
        if read {
            self.nav.closed.remove(&unread_key);
            self.nav.closed.insert(read_key);
        } else {
            self.nav.closed.remove(&read_key);
            self.nav.closed.insert(unread_key);
        }
    }

    pub(super) async fn inbox_read_state(&mut self, id: &str, read: bool) -> Result<()> {
        ensure!(
            self.page == Page::Inbox && self.places().iter().any(|d| d.id == id),
            "This conversation is no longer in your inbox."
        );
        if self.preview {
            self.preview_read_state(id, read);
        } else {
            let service = self.service()?;
            if read {
                service.mark_read(PRIVATE, id, true);
            } else {
                let page = service.history(PRIVATE, id, Span::Latest).await?;
                self.mark_latest_unread(PRIVATE, id, &page.messages)?;
            }
        }
        Ok(())
    }

    pub(super) fn mark_latest_unread(
        &self,
        guild: &str,
        id: &str,
        items: &[Message],
    ) -> Result<()> {
        let order = items
            .iter()
            .filter(|m| m.channel_id == id)
            .map(Message::order)
            .max()
            .context("There are no messages to mark unread.")?;
        let last = items
            .iter()
            .filter(|m| m.channel_id == id)
            .map(Message::order)
            .filter(|o| *o < order)
            .max()
            .unwrap_or(((order >> 22).saturating_sub(1)) << 22);
        let marks = self
            .snapshot
            .servers
            .iter()
            .find(|g| g.id == guild)
            .map(|g| g.marks.as_ref());
        let mentions = items
            .iter()
            .filter(|m| {
                m.channel_id == id
                    && m.order() > last
                    && m.author_id != self.me()
                    && (guild == PRIVATE || m.mentions_me(self.me(), marks))
            })
            .count() as u64;
        self.service()?.mark_unread(id, last, mentions, true);
        Ok(())
    }

    pub(super) fn tool_permissions(&self) -> (bool, Deletes) {
        if self.room.request || self.me().is_empty() {
            return (false, Deletes::None);
        }
        if self.preview || self.room.guild == PRIVATE {
            return (true, Deletes::Own);
        }
        if let Some(server) = self
            .snapshot
            .servers
            .iter()
            .find(|s| s.id == self.room.guild)
            && let Some(channel) = server.channel(&self.room.id)
            && channel.marks.access != Access::Hidden
        {
            let roles = &server.marks.roles;
            let allowed = server.channel_permissions(channel.channel, self.me(), roles);
            return (
                allowed & permission::PIN_MESSAGES != 0,
                server.deletes(channel.channel, self.me(), roles),
            );
        }
        (false, Deletes::None)
    }

    pub(super) async fn message_tool(&mut self, id: &str, action: &str) -> Result<()> {
        ensure!(self.page == Page::Chat, "Open the conversation first.");
        let post = self
            .posts
            .iter()
            .find(|m| m.id == id && m.channel_id == self.room.id)
            .context("This message is no longer in the conversation.")?;
        let (may_pin, deletes) = self.tool_permissions();
        let can_share = !post.ephemeral;
        match action {
            "copy" => {
                ensure!(
                    !self.snapshot.blocked.contains(&post.author_id)
                        && !self.snapshot.ignored.contains(&post.author_id),
                    "This message is hidden."
                );
                ensure!(
                    !post.content.is_empty(),
                    "There is no message text to copy."
                );
                self.platform.strings("copyMessage", &[&post.content])?;
                self.notice = "Message copied.".into();
            }
            "pin" | "unpin" => {
                ensure!(
                    may_pin && can_share && post.system.is_none(),
                    "You cannot pin messages here."
                );
                let pinned = action == "pin";
                if post.pinned != pinned {
                    if !self.preview {
                        self.service()?.pin(&self.room.id, id, pinned).await?;
                    }
                    if let Some(m) = self.posts.iter_mut().find(|m| m.id == id) {
                        m.pinned = pinned;
                    }
                }
                self.notice = if pinned {
                    "Message pinned."
                } else {
                    "Message unpinned."
                }
                .into();
            }
            "delete" => {
                ensure!(
                    can_share && deletes.allow(post, self.me()),
                    "You cannot delete this message."
                );
                if !self.preview {
                    self.service()?.delete(&self.room.id, id).await?;
                }
                self.posts.retain(|m| m.id != id);
                if self.drafts.get(&self.room.id).is_some_and(|c| {
                    c.edit.as_deref() == Some(id) || c.reply.as_deref() == Some(id)
                }) {
                    self.cancel_compose();
                }
            }
            _ => return Ok(()),
        }
        if self.preview && action != "copy" {
            self.samples
                .insert(self.room.id.clone(), self.posts.clone());
        }
        self.collapse = true;
        Ok(())
    }

    pub(super) fn sync_tools(&mut self) {
        if self.tools.source != self.room.id
            || self.tools.epoch != self.epoch
            || self.page != Page::Chat
        {
            if self.tools.open() || !self.tools.jump.is_empty() || self.tools.past {
                self.tools.close();
                self.tools.jump.clear();
                self.tools.past = false;
            }
        }
        if !std::mem::take(&mut self.tools.dirty) {
            return;
        }
        let scope = self.room.name.clone();
        let can_search_server = self.room.guild != PRIVATE;
        let mut status = self.tools.note.clone();
        let more = self.tools.more;
        let busy = self.tools.busy;
        let mode = self.tools.mode;
        let jump = self.tools.jump.clone();
        let past = self.tools.past;
        let mut rows = Vec::new();
        if self.tools.open() {
            for hit in &self.tools.hits {
                let Some(channel) = self.hit_channel(&hit.channel_id) else {
                    continue;
                };
                if self.snapshot.blocked.contains(&hit.author_id)
                    || self.snapshot.ignored.contains(&hit.author_id)
                {
                    continue;
                }
                let server = self
                    .snapshot
                    .servers
                    .iter()
                    .find(|s| s.id == self.room.guild);
                let text = if let Some(system) = &hit.system {
                    system.clone()
                } else if let Some(service) = &self.discord {
                    service.render(server.map(|g| g.server.as_ref()), &hit.content)
                } else {
                    hit.content.clone()
                };
                let mut text = crate::spoiler::hide(&text)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if text.is_empty() {
                    text = hit
                        .attachments
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                }
                if text.is_empty() {
                    text = if hit.poll.is_some() {
                        "Poll"
                    } else if hit.voice.is_some() {
                        "Voice message"
                    } else {
                        "Message"
                    }
                    .into();
                }
                let time = hit.timestamp();
                let date = crate::clock::date(time)
                    .map_or_else(String::new, |d| d.format("%b %e, %Y").to_string());
                let picture = self
                    .discord
                    .as_ref()
                    .and_then(|d| {
                        self.pictures
                            .get(&d.avatar(Some(&self.room.guild), &hit.author_id))
                    })
                    .and_then(Option::clone);
                rows.push((
                    hit.id.clone(),
                    hit.channel_id.clone(),
                    channel,
                    hit.author.clone(),
                    crate::view::initial(&hit.author),
                    text.chars().take(250).collect::<String>(),
                    format!("{} · {}", date.trim(), crate::clock::time(time)),
                    picture,
                    if self.preview {
                        crate::preview::avatar(&hit.author_id)
                    } else {
                        ""
                    },
                ));
            }
        }
        if !self.tools.hits.is_empty() && rows.is_empty() {
            status = "No visible messages.".into();
        }
        let _ = self.ui.upgrade_in_event_loop(move |w| {
            let rows = rows
                .into_iter()
                .map(
                    |(id, channel, label, author, initial, body, time, picture, preview)| {
                        crate::ToolHit {
                            id: id.into(),
                            channel: channel.into(),
                            channel_label: label.into(),
                            author: author.into(),
                            initial: initial.into(),
                            body: body.into(),
                            time: time.into(),
                            avatar: crate::preview::picture(preview)
                                .or_else(|| picture.map(|p| p.image()))
                                .unwrap_or_default(),
                        }
                    },
                )
                .collect::<Vec<_>>();
            w.set_tool_hits(std::rc::Rc::new(slint::VecModel::from(rows)).into());
            w.set_tool_mode(mode.into());
            w.set_tool_note(status.into());
            w.set_tool_channel(scope.into());
            w.set_tool_server(can_search_server);
            w.set_tool_busy(busy);
            w.set_tool_more(more);
            w.set_jump_target(jump.into());
            w.set_tool_past(past);
        });
    }

    fn hit_channel(&self, id: &str) -> Option<String> {
        if self.preview || self.room.guild == PRIVATE {
            if id == self.room.id {
                return Some(self.room.name.clone());
            }
            if self.preview && self.room.guild != PRIVATE {
                return self
                    .channels(&self.room.guild)
                    .into_iter()
                    .find(|c| c.id == id && c.kind != "voice")
                    .map(|c| c.name);
            }
            return None;
        }
        let server = self
            .snapshot
            .servers
            .iter()
            .find(|s| s.id == self.room.guild)?;
        let channel = server.channel(id)?;
        (channel.marks.access != Access::Hidden && channel.forum.is_none())
            .then(|| channel.name.clone())
    }

    pub(super) async fn tools_event(&mut self, event: Event) -> Result<()> {
        if self.page != Page::Chat {
            return Ok(());
        }
        match event {
            Event::Loaded(generation, result)
                if generation == self.tools.generation
                    && self.tools.source == self.room.id
                    && self.tools.epoch == self.epoch =>
            {
                self.tools.job.take();
                self.tools.busy = false;
                self.tools.dirty = true;
                match result {
                    Ok(found) => self.tools_result(found),
                    Err(e) => self.tools.note = e,
                }
            }
            Event::Loaded(..) => {}
            Event::Input(action, text, scope) => match action.as_str() {
                "search" | "pins" => {
                    self.details.close();
                    self.tools.close();
                    self.tools.whole_server = false;
                    self.tools.mode = if action == "pins" { "pins" } else { "search" };
                    self.tools.source = self.room.id.clone();
                    self.tools.epoch = self.epoch;
                    self.tools.jump.clear();
                    self.collapse = true;
                    if action == "pins" {
                        self.start_tools_query()?;
                    }
                }
                "submit" if self.tools.mode == "search" => {
                    self.tools.query = text.trim().to_owned();
                    self.tools.whole_server = scope == "server" && self.room.guild != PRIVATE;
                    self.tools.more = false;
                    self.tools.offset = 0;
                    self.tools.total = 0;
                    self.tools.hits.clear();
                    self.start_tools_query()?;
                }
                "more" if !self.tools.busy && self.tools.more => self.start_tools_query()?,
                "retry" if self.tools.open() && !self.tools.busy => self.start_tools_query()?,
                "close" => self.tools.close(),
                "jump" if self.tools.open() => {
                    if let Err(error) = self.jump_to_hit(&text, &scope).await {
                        self.tools.note = error.to_string();
                        self.tools.dirty = true;
                    }
                }
                "latest" => self.return_to_present().await?,
                _ => {}
            },
        }
        Ok(())
    }

    fn start_tools_query(&mut self) -> Result<()> {
        self.tools.job.take();
        self.tools.generation = self.tools.generation.wrapping_add(1);
        self.tools.dirty = true;
        self.tools.note.clear();
        let pins = self.tools.mode == "pins";
        let query = self.tools.query.clone();
        if !pins && query.is_empty() {
            self.tools.busy = false;
            return Ok(());
        }
        if self.preview {
            let mut names = HashMap::from([(self.room.id.clone(), self.room.name.clone())]);
            let mut messages = self.posts.clone();
            if self.tools.whole_server && !pins {
                for channel in self.channels(&self.room.guild) {
                    if channel.kind == "voice" || channel.id == self.room.id {
                        continue;
                    }
                    names.insert(channel.id.clone(), channel.name);
                    let items = self
                        .samples
                        .entry(channel.id.clone())
                        .or_insert_with(|| crate::preview::messages(&channel.id));
                    messages.extend(items.iter().cloned());
                }
            }
            let result = preview_page(&messages, &query, pins, self.tools.offset, &names);
            match result {
                Ok(page) => self.tools_result(page),
                Err(e) => self.tools.note = e.to_string(),
            }
            return Ok(());
        }
        let service = self.service()?;
        self.tools.busy = true;
        let guild = self.room.guild.clone();
        let channel = self.room.id.clone();
        let tx = self.tx.clone();
        let generation = self.tools.generation;
        let offset = self.tools.offset;
        let all = self.tools.whole_server;
        self.tools.job = Some(self.runtime.spawn(async move {
            let response = if pins {
                service.pins(&guild, &channel).await.map(|messages| Found {
                    total: messages.len() as u32,
                    messages,
                })
            } else if all {
                service.search(&guild, None, &query, offset).await
            } else {
                service
                    .search_in_channel(&guild, &channel, &query, offset)
                    .await
            };
            let _ = tx.send(Command::Tools(Event::Loaded(
                generation,
                response.map_err(|e| e.to_string()),
            )));
        }));
        Ok(())
    }

    fn tools_result(&mut self, found: Found) {
        let count = found.messages.len() as u32;
        self.tools.total = found.total;
        self.tools.offset = self.tools.offset.saturating_add(count);
        self.tools.more =
            self.tools.mode == "search" && count != 0 && self.tools.offset < found.total;
        let mut ids: HashSet<_> = self.tools.hits.iter().map(|m| m.id.clone()).collect();
        self.tools.hits.extend(
            found
                .messages
                .into_iter()
                .filter(|m| ids.insert(m.id.clone())),
        );
        self.tools.note = if self.tools.hits.is_empty() {
            if self.tools.mode == "pins" {
                "No pinned messages yet.".into()
            } else {
                "No messages found.".into()
            }
        } else if self.tools.mode == "search" {
            format!(
                "{} {}",
                found.total,
                if found.total == 1 {
                    "result"
                } else {
                    "results"
                }
            )
        } else {
            String::new()
        };
        self.tools.dirty = true;
    }

    async fn jump_to_hit(&mut self, id: &str, channel: &str) -> Result<()> {
        ensure!(
            self.tools.hits.iter().any(|m| m.id == id
                && m.channel_id == channel
                && !self.snapshot.blocked.contains(&m.author_id)
                && !self.snapshot.ignored.contains(&m.author_id)),
            "This result is unavailable."
        );
        let name = self
            .hit_channel(channel)
            .context("This channel is no longer available.")?;
        let same = self.room.id == channel;
        if self.preview {
            let messages = if same {
                self.posts.clone()
            } else {
                self.samples
                    .get(channel)
                    .cloned()
                    .unwrap_or_else(|| crate::preview::messages(channel))
            };
            ensure!(
                messages.iter().any(|m| m.id == id),
                "This message was removed."
            );
            if !same {
                self.cancel_pick();
                self.samples
                    .insert(self.room.id.clone(), self.posts.clone());
                self.room = Room {
                    guild: self.room.guild.clone(),
                    id: channel.into(),
                    name,
                    writable: true,
                    ..Default::default()
                };
                self.posts = messages;
                self.writing = true;
                self.oldest = None;
                self.older = false;
            }
        } else if !same || !self.posts.iter().any(|m| m.id == id) {
            let service = self.service()?;
            let page = service
                .history(&self.room.guild, channel, Span::Around(id))
                .await?;
            ensure!(
                page.messages.iter().any(|m| m.id == id),
                "This message was removed."
            );
            if !same {
                self.cancel_pick();
                self.room = Room {
                    guild: self.room.guild.clone(),
                    id: channel.into(),
                    name,
                    ..Default::default()
                };
                self.update_room();
                self.writing = true;
                service.view(&self.room.guild, Some(channel));
            }
            self.posts = page.messages;
            self.posts.sort_by_key(Message::order);
            self.oldest = page.oldest;
            self.older = page.full && page.oldest.is_some();
            self.tools.past = !page.newest;
        }
        self.tools.close();
        self.tools.source = channel.into();
        self.tools.jump = id.into();
        self.collapse = true;
        self.notice.clear();
        Ok(())
    }

    pub(super) async fn return_to_present(&mut self) -> Result<()> {
        if self.tools.past && !self.preview {
            self.history(None).await?;
        }
        self.tools.close();
        self.tools.past = false;
        self.tools.jump.clear();
        self.tail += 1;
        Ok(())
    }
}

fn preview_page(
    messages: &[Message],
    text: &str,
    pins: bool,
    offset: u32,
    channels: &HashMap<String, String>,
) -> Result<Found> {
    let query = Query::parse(text).map_err(anyhow::Error::msg)?;
    let words = query.content.to_lowercase();
    let mut matches: Vec<_> = messages
        .iter()
        .filter(|m| {
            if pins {
                return m.pinned;
            }
            let author = m.author.to_lowercase();
            let content = m.content.to_lowercase();
            content.contains(&words)
                && query
                    .from
                    .iter()
                    .all(|name| name.eq_ignore_ascii_case(&author) || name == &m.author_id)
                && query.mentions.iter().all(|name| {
                    m.mentions.contains(name)
                        || content.contains(&format!("@{}", name.to_lowercase()))
                })
                && query.channels.iter().all(|name| {
                    name == &m.channel_id
                        || channels
                            .get(&m.channel_id)
                            .is_some_and(|channel| name.eq_ignore_ascii_case(channel))
                })
                && query.pinned.is_none_or(|p| p == m.pinned)
                && query.min_id.is_none_or(|id| m.order() >= id)
                && query.max_id.is_none_or(|id| m.order() < id)
                && query.has.iter().all(|kind| match *kind {
                    "link" => content.contains("http://") || content.contains("https://"),
                    "file" => !m.attachments.is_empty(),
                    "image" => m.attachments.iter().any(|f| f.image),
                    "video" => m.attachments.iter().any(|f| f.video_url.is_some()),
                    "sound" => {
                        m.voice.is_some()
                            || m.attachments
                                .iter()
                                .any(|f| f.name.ends_with(".ogg") || f.name.ends_with(".mp3"))
                    }
                    "embed" => !m.embeds.is_empty(),
                    "poll" => m.poll.is_some(),
                    "snapshot" => m.forwarded,
                    _ => false,
                })
        })
        .collect();
    matches.sort_by_key(|m| std::cmp::Reverse(m.order()));
    let total = matches.len() as u32;
    let limit = if pins { matches.len() } else { 25 };
    let messages = matches
        .into_iter()
        .skip(offset as usize)
        .take(limit)
        .cloned()
        .collect();
    Ok(Found { total, messages })
}
