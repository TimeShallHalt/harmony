use super::*;
use crate::preview;
use std::collections::HashSet;
use yapfast_discord::member_list::Presence;

#[derive(Default)]
pub(super) struct Navigation {
    pub pages: Vec<Page>,
    pub query: String,
    pub compose_only: bool,
    pub closed: HashSet<String>,
    pub recent: HashMap<String, (String, String)>,
    pub alerts: Vec<Destination>,
    pub pending_friends: Vec<Destination>,
    pub appearance: String,
    pub local_profile: discord::profile::OwnProfile,
    pub local_presence: String,
    pub files: HashMap<String, StagedFile>,
}
fn presence(value: Option<Presence>) -> String {
    match value.map(Presence::base) {
        Some(Presence::Online | Presence::Mobile) => "online",
        Some(Presence::Idle) => "idle",
        Some(Presence::DoNotDisturb) => "dnd",
        Some(_) => "offline",
        None => "",
    }
    .into()
}
fn short(text: &str) -> String {
    crate::spoiler::hide(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}
impl Shell {
    pub(super) fn visit(&mut self, page: Page) {
        if self.page != page {
            self.nav.pages.push(self.page);
            self.page = page;
        }
        self.nav.query.clear();
    }
    pub(super) fn guilds(&self) -> Vec<Destination> {
        if self.preview {
            return preview::servers()
                .into_iter()
                .chain(self.create.servers.iter().cloned())
                .collect();
        }
        self.snapshot
            .servers
            .iter()
            .map(|g| {
                let mut d = Destination::new(&g.id, &g.name, &view::initial(&g.name), "", false)
                    .avatar(g.icon.as_deref());
                for mark in &g.marks.channels {
                    if mark.access != Access::Hidden && !mark.muted && !g.marks.muted {
                        d.unread |= mark.unread;
                        d.count = d
                            .count
                            .saturating_add(mark.mentions.min(i32::MAX as u64) as i32);
                    }
                }
                d
            })
            .collect()
    }
    fn conversations(&self, requests: bool) -> Vec<Destination> {
        if self.preview {
            if requests {
                return vec![];
            }
            let mut rows = self.create.groups.clone();
            rows.extend(preview::dms());
            for d in &mut rows {
                if self.nav.closed.contains(&format!("read/{}", d.id)) {
                    d.unread = false;
                    d.count = 0;
                } else if self.nav.closed.contains(&format!("unread/{}", d.id)) {
                    d.unread = true;
                }
                d.presence = match d.id.as_str() {
                    "co-op" => "",
                    "alex" => "idle",
                    "ada" => "offline",
                    _ => "online",
                }
                .into();
                if let Some((_, summary)) = self.nav.recent.get(&d.id) {
                    d.detail.clone_from(summary);
                }
            }
            return rows;
        }
        self.snapshot
            .conversations
            .iter()
            .filter(|c| c.request == requests)
            .map(|c| {
                let detail = self
                    .nav
                    .recent
                    .get(&c.id)
                    .map(|(_, s)| s.clone())
                    .or_else(|| c.preview.as_ref().map(|p| short(&p.text)))
                    .filter(|v| !v.is_empty())
                    .or_else(|| c.activity.clone())
                    .unwrap_or_else(|| {
                        if c.group {
                            format!("{} members", c.people)
                        } else if c.request {
                            "Message request".into()
                        } else {
                            "Direct message".into()
                        }
                    });
                let mut d = Destination::new(
                    &c.id,
                    &c.name,
                    &view::initial(&c.name),
                    &detail,
                    c.unread && !c.muted,
                )
                .avatar(c.avatar.as_deref());
                d.presence = presence(c.presence);
                d.count = if c.muted {
                    0
                } else {
                    c.mentions.min(i32::MAX as u64) as i32
                };
                d
            })
            .collect()
    }
    pub(super) fn people(&self) -> Vec<Destination> {
        if self.preview {
            return self
                .nav
                .pending_friends
                .iter()
                .cloned()
                .chain(preview::people())
                .collect();
        }
        self.snapshot
            .friend_requests
            .iter()
            .map(|f| {
                Destination::new(
                    &f.user_id,
                    &f.name,
                    &view::initial(&f.name),
                    if f.incoming {
                        "Incoming friend request"
                    } else {
                        "Outgoing request"
                    },
                    false,
                )
                .avatar(Some(&f.avatar))
                .request(f.incoming)
            })
            .chain(self.snapshot.friend_list.iter().map(|f| {
                let mut d = Destination::new(
                    &f.user_id,
                    &f.name,
                    &view::initial(&f.name),
                    f.activity.as_deref().unwrap_or("Direct message"),
                    false,
                )
                .avatar(Some(&f.avatar));
                d.presence = presence(f.presence);
                d
            }))
            .collect()
    }
    pub(super) fn activities(&self) -> Vec<Destination> {
        let mut rows = if self.preview {
            preview::people()
                .into_iter()
                .filter(|p| p.detail.starts_with("Playing") || p.detail.starts_with("Listening"))
                .collect()
        } else {
            self.snapshot
                .friend_list
                .iter()
                .filter_map(|f| {
                    let a = f
                        .activities
                        .iter()
                        .find_map(|a| a.line())
                        .or_else(|| f.activity.clone())?;
                    (!a.trim().is_empty()).then(|| {
                        Destination::new(&f.user_id, &f.name, &view::initial(&f.name), &a, false)
                            .avatar(Some(&f.avatar))
                    })
                })
                .take(16)
                .collect::<Vec<_>>()
        };
        for d in &mut rows {
            d.id = format!("person/{}", d.id);
        }
        rows
    }
    pub(super) fn channels(&self, server: &str) -> Vec<Destination> {
        let mut out = vec![];
        if self.preview {
            out.extend(self.create.channels(server));
            for mut d in preview::channels(server) {
                d.kind = if d.mark == "♪" { "voice" } else { "text" }.into();
                d.category = preview::category(&d.id).into();
                out.push(d);
            }
        } else if let Some(g) = self.snapshot.servers.iter().find(|g| g.id == server) {
            let mut channels: Vec<_> = g
                .text_channels()
                .chain(g.voice_channels())
                .chain(g.threads())
                .filter(|c| c.marks.access != Access::Hidden)
                .collect();
            channels.sort_by_key(|c| {
                let parent = c.parent.as_deref().and_then(|p| g.categories.get(p));
                (parent.map_or(-1, |c| c.position), c.position, c.id.clone())
            });
            for c in channels {
                let category = c
                    .parent
                    .as_deref()
                    .and_then(|p| g.categories.get(p))
                    .map(|v| v.name.as_str())
                    .unwrap_or(if c.thread.is_some() {
                        "Threads"
                    } else {
                        "Channels"
                    });
                let kind = if c.forum.is_some() {
                    "forum"
                } else if c.thread.is_some() {
                    "thread"
                } else if c.channel_type == Some(2) || c.stage {
                    "voice"
                } else if c.announcement {
                    "news"
                } else if c.marks.access == Access::Locked {
                    "locked"
                } else {
                    "text"
                };
                let mut d = Destination::new(
                    &c.id,
                    &c.name,
                    if kind == "voice" { "♪" } else { "#" },
                    category,
                    c.marks.unread && !c.marks.muted,
                );
                d.kind = kind.into();
                d.category = category.into();
                d.count = c.marks.mentions.min(i32::MAX as u64) as i32;
                out.push(d);
            }
        }
        out
    }
    fn matches(&self, d: &Destination) -> bool {
        let q = self.nav.query.trim().trim_start_matches('#').to_lowercase();
        q.is_empty() || d.name.to_lowercase().contains(&q) || d.detail.to_lowercase().contains(&q)
    }
    pub(super) fn places(&self) -> Vec<Destination> {
        match self.page {
            Page::Inbox => self.conversations(false),
            Page::Requests => self.conversations(true),
            Page::Friends => self
                .people()
                .into_iter()
                .filter(|d| self.matches(d))
                .collect(),
            Page::Notifications => {
                if self.preview {
                    preview::activity()
                        .into_iter()
                        .filter(|p| !self.nav.closed.contains(&format!("alert/{}", p.id)))
                        .collect()
                } else {
                    self.nav.alerts.clone()
                }
            }
            Page::Channels => {
                let mut ordered = Vec::new();
                let mut groups = Vec::<(String, Vec<Destination>)>::new();
                for d in self.channels(&self.server) {
                    if let Some((_, items)) =
                        groups.iter_mut().find(|(name, _)| name == &d.category)
                    {
                        items.push(d);
                    } else {
                        groups.push((d.category.clone(), vec![d]));
                    }
                }
                for (name, items) in groups {
                    let key = format!("category/{}/{}", self.server, name);
                    let closed = self.nav.closed.contains(&key);
                    let mut d = Destination::new(
                        &key,
                        &name,
                        if closed { "closed" } else { "open" },
                        "",
                        false,
                    );
                    d.kind = "category".into();
                    ordered.push(d);
                    if !closed {
                        ordered.extend(items);
                    }
                }
                ordered
            }
            Page::QuickFind => {
                let mut rows = self.conversations(false);
                let mut people = self.people();
                people.retain(|p| p.relationship == 0);
                for p in &mut people {
                    p.id = format!("person/{}", p.id);
                }
                if self.nav.compose_only {
                    return people.into_iter().filter(|d| self.matches(d)).collect();
                }
                if self.nav.query.trim().is_empty() && self.server.is_empty() {
                    return rows;
                }
                for g in self.guilds() {
                    if self.nav.query.trim().is_empty() && self.server != g.id {
                        continue;
                    }
                    for mut d in self.channels(&g.id) {
                        d.id = format!("channel/{}/{}", g.id, d.id);
                        d.detail = g.name.clone();
                        rows.push(d);
                    }
                }
                rows.into_iter()
                    .filter(|d| self.matches(d))
                    .take(80)
                    .collect()
            }
            _ => vec![],
        }
    }
    pub(super) fn room_muted(&self) -> bool {
        if self.preview {
            return self.nav.closed.contains(&format!("mute/{}", self.room.id));
        }
        if self.room.guild == PRIVATE {
            self.snapshot
                .conversations
                .iter()
                .find(|c| c.id == self.room.id)
                .is_some_and(|c| c.muted)
        } else {
            self.snapshot
                .servers
                .iter()
                .find(|g| g.id == self.room.guild)
                .and_then(|g| g.channel(&self.room.id))
                .is_some_and(|c| c.marks.muted)
        }
    }
    pub(super) fn request_total(&self) -> i32 {
        self.snapshot
            .conversations
            .iter()
            .filter(|c| c.request)
            .count() as i32
    }
    pub(super) fn remember_message(&mut self, m: &Message) {
        if self.snapshot.blocked.contains(&m.author_id)
            || self.snapshot.ignored.contains(&m.author_id)
        {
            return;
        }
        let summary = if m.content.is_empty() {
            if m.attachments.is_empty() {
                return;
            }
            "Attachment".into()
        } else {
            short(&m.content)
        };
        if self
            .nav
            .recent
            .get(&m.channel_id)
            .is_some_and(|(id, _)| id.parse::<u64>().unwrap_or(0) > m.order())
        {
            return;
        }
        if self.nav.recent.len() >= 128 && !self.nav.recent.contains_key(&m.channel_id) {
            if let Some(earliest) = self
                .nav
                .recent
                .iter()
                .min_by_key(|(_, v)| v.0.parse::<u64>().unwrap_or(0))
                .map(|(k, _)| k.clone())
            {
                self.nav.recent.remove(&earliest);
            }
        }
        self.nav
            .recent
            .insert(m.channel_id.clone(), (m.id.clone(), summary));
    }
    pub(super) fn appearance(&self) -> &str {
        if self.nav.appearance.is_empty() {
            "dark"
        } else {
            &self.nav.appearance
        }
    }
    pub(super) fn theme_path(&self) -> std::path::PathBuf {
        self.platform
            .cache
            .with_file_name("files")
            .join("appearance")
    }
}
