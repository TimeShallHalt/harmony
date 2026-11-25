use super::{Command, Destination, Page, Room, Sender, Shell};
use crate::{preview, runtime::BackgroundTask};
use anyhow::{Context, Result, ensure};
use std::collections::HashMap;
use yapfast_discord::{
    invite::{self, JoinFrom},
    ready::PRIVATE,
    service::ChannelSettings,
};

pub(crate) enum Event {
    Input(String, String, String),
    Reviewed(u64, Result<invite::Preview, String>),
    Challenge(u64, String),
    Landing(u64, String, String),
    Finished(u64, Result<Outcome, String>),
}

pub(crate) enum Outcome {
    Server {
        id: String,
        channel: Option<String>,
        form: bool,
    },
    Group {
        id: String,
        name: String,
        note: String,
    },
}

#[derive(Default)]
pub(super) struct Create {
    mode: &'static str,
    epoch: u64,
    generation: u64,
    busy: bool,
    writing: bool,
    dirty: bool,
    note: String,
    query: String,
    chosen: Vec<String>,
    invite: Option<invite::Preview>,
    challenge: String,
    job: Option<BackgroundTask<()>>,
    sequence: u32,
    pub(super) servers: Vec<Destination>,
    pub(super) groups: Vec<Destination>,
    pub(super) members: HashMap<String, Vec<(String, String)>>,
}

impl Create {
    pub(super) fn open(&self) -> bool {
        !self.mode.is_empty()
    }

    fn cancel_read(&mut self) {
        self.job.take();
        self.generation = self.generation.wrapping_add(1);
        self.challenge.clear();
        self.busy = false;
        self.writing = false;
    }

    fn show(&mut self, mode: &'static str) {
        self.cancel_read();
        self.mode = mode;
        self.note.clear();
        self.dirty = true;
        if mode != "review" {
            self.invite = None;
        }
        if mode != "group" {
            self.chosen.clear();
            self.query.clear();
        }
    }

    pub(super) fn invalidate(&mut self) {
        self.dirty |= self.open();
    }

    pub(super) fn back(&mut self) {
        if self.writing && self.challenge.is_empty() {
            return;
        }
        match self.mode {
            "review" => self.show("invite"),
            "invite" | "create" => self.show("servers"),
            _ => self.show(""),
        }
    }

    pub(super) fn channels(&self, server: &str) -> Vec<Destination> {
        if !self.servers.iter().any(|g| g.id == server) {
            return vec![];
        }
        let mut room = Destination::new(
            &format!("{server}-general"),
            "general",
            "#",
            "Text channel",
            false,
        );
        room.category = "Text Channels".into();
        room.kind = "text".into();
        vec![room]
    }
}

pub(super) fn bind(w: &crate::MobileWindow, tx: Sender) {
    w.on_create_command(move |a, b, c| {
        let _ = tx.send(Command::Create(Event::Input(a.into(), b.into(), c.into())));
    });
}

impl Shell {
    fn create_friends(&self) -> Vec<Destination> {
        self.people()
            .into_iter()
            .filter(|u| {
                u.relationship == 0
                    && u.id != self.me()
                    && !self.snapshot.blocked.contains(&u.id)
                    && !self.snapshot.ignored.contains(&u.id)
            })
            .collect()
    }

    pub(super) async fn creation(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Input(action, id, extra) => {
                if matches!(self.page, Page::Welcome | Page::Linking) {
                    return Ok(());
                }
                if let Err(e) = self.create_input(&action, &id, &extra).await {
                    self.create.note = e.to_string();
                    self.create.dirty = true;
                }
            }
            Event::Reviewed(generation, result)
                if generation == self.create.generation && self.create.mode == "invite" =>
            {
                self.create.job.take();
                self.create.busy = false;
                match result {
                    Ok(p) => {
                        self.create.mode = "review";
                        self.create.invite = Some(p);
                        self.create.note.clear();
                    }
                    Err(e) => self.create.note = e,
                }
            }
            Event::Challenge(generation, url)
                if generation == self.create.generation && self.create.writing =>
            {
                self.create.challenge = url;
                self.create.note = "Complete Discord’s verification in your browser.".into();
                if let Err(e) = self.platform.strings("browser", &[&self.create.challenge]) {
                    self.create.note = e.to_string();
                }
            }
            Event::Landing(generation, guild, channel)
                if generation == self.create.generation
                    && self.page == Page::Channels
                    && self.server == guild =>
            {
                self.create.job.take();
                if let Some(service) = &self.discord {
                    self.snapshot = service.snapshots().borrow().clone();
                }
                if self.channels(&guild).into_iter().any(|c| c.id == channel) {
                    self.choose(&format!("channel/{guild}/{channel}")).await?;
                }
            }
            Event::Finished(generation, result)
                if generation == self.create.generation && self.create.writing =>
            {
                self.create.job.take();
                self.create.busy = false;
                self.create.writing = false;
                self.create.challenge.clear();
                match result {
                    Ok(outcome) => self.finish_create(outcome),
                    Err(e) => self.create.note = e,
                }
            }
            _ => return Ok(()),
        }
        self.create.dirty = true;
        Ok(())
    }

    async fn create_input(&mut self, action: &str, id: &str, _extra: &str) -> Result<()> {
        if action == "back" {
            self.create.back();
            return Ok(());
        }
        if action == "verify" && !self.create.challenge.is_empty() {
            self.platform
                .strings("browser", &[&self.create.challenge])?;
            return Ok(());
        }
        if self.create.busy {
            return Ok(());
        }
        match action {
            "server" | "group" if !self.create.open() => {
                self.create.epoch = self.epoch;
                self.create.show(if action == "server" {
                    "servers"
                } else {
                    "group"
                });
            }
            "create" | "invite" if self.create.mode == "servers" => {
                self.create.show(if action == "create" {
                    "create"
                } else {
                    "invite"
                })
            }
            "filter" if self.create.mode == "group" => self.create.query = id.to_owned(),
            "toggle" if self.create.mode == "group" => {
                if let Some(at) = self.create.chosen.iter().position(|u| u == id) {
                    self.create.chosen.remove(at);
                } else {
                    ensure!(
                        self.create_friends().iter().any(|p| p.id == id),
                        "This friend is no longer available."
                    );
                    ensure!(
                        self.create.chosen.len() < 9,
                        "A group can have up to 10 people including you."
                    );
                    self.create.chosen.push(id.into());
                }
                self.create.note.clear();
            }
            "review" if self.create.mode == "invite" => self.review_invite(id)?,
            "submit" if self.create.mode == "create" => {
                let name = id.trim();
                ensure!(
                    (2..=100).contains(&name.chars().count()),
                    "Enter a server name between 2 and 100 characters."
                );
                if self.preview {
                    self.create.sequence = self.create.sequence.wrapping_add(1);
                    let server = format!("local-server-{}", self.create.sequence);
                    self.create.servers.push(Destination::new(
                        &server,
                        name,
                        &crate::view::initial(name),
                        "Server",
                        false,
                    ));
                    self.samples.insert(format!("{server}-general"), vec![]);
                    self.finish_create(Outcome::Server {
                        id: server,
                        channel: None,
                        form: false,
                    });
                    return Ok(());
                }
                let service = self.service()?;
                let name = name.to_string();
                let tx = self.tx.clone();
                let generation = self.begin_create_write();
                self.create.job = Some(self.runtime.spawn(async move {
                    let result = service
                        .create_server(&name, |url| {
                            let _ =
                                tx.send(Command::Create(Event::Challenge(generation, url.into())));
                        })
                        .await
                        .map(|id| Outcome::Server {
                            id,
                            channel: None,
                            form: false,
                        });
                    let _ = tx.send(Command::Create(Event::Finished(
                        generation,
                        result.map_err(|e| e.to_string()),
                    )));
                }));
            }
            "submit" if self.create.mode == "review" => {
                let invite = self
                    .create
                    .invite
                    .clone()
                    .context("Review the invite first.")?;
                if self.preview {
                    self.finish_create(Outcome::Server {
                        id: invite.guild,
                        channel: Some(invite.channel),
                        form: false,
                    });
                    return Ok(());
                }
                let service = self.service()?;
                let tx = self.tx.clone();
                let generation = self.begin_create_write();
                self.create.job = Some(self.runtime.spawn(async move {
                    let result = service
                        .join_invite(&invite, JoinFrom::Dialog, |url| {
                            let _ =
                                tx.send(Command::Create(Event::Challenge(generation, url.into())));
                        })
                        .await
                        .map(|joined| Outcome::Server {
                            id: joined.guild,
                            channel: Some(joined.channel),
                            form: joined.verification_form,
                        });
                    let _ = tx.send(Command::Create(Event::Finished(
                        generation,
                        result.map_err(|e| e.to_string()),
                    )));
                }));
            }
            "submit" if self.create.mode == "group" => self.create_group_chat(id)?,
            _ => {}
        }
        self.create.dirty = true;
        Ok(())
    }

    fn begin_create_write(&mut self) -> u64 {
        self.create.cancel_read();
        self.create.busy = true;
        self.create.writing = true;
        self.create.note.clear();
        self.create.generation
    }

    fn review_invite(&mut self, input: &str) -> Result<()> {
        let typed = input.trim();
        // Pass only an invite code to the Discord API. No user URL is opened.
        let code =
            invite::typed_code(typed).context("Enter a Discord invite link or invite code.")?;
        if typed.contains("://") {
            ensure!(
                invite::link_code(typed).is_some(),
                "This link does not point to a Discord invite."
            );
        }
        if self.preview {
            let guild = self
                .guilds()
                .into_iter()
                .find(|g| g.id == code)
                .context("This preview invite is unknown. Try night-shift.")?;
            self.create.invite = Some(invite::Preview {
                code: code.into(),
                guild: guild.id.clone(),
                name: guild.name,
                icon: None,
                channel: if guild.id == preview::FEATURED_SERVER {
                    preview::FEATURED_CHANNEL.into()
                } else {
                    String::new()
                },
                channel_name: String::new(),
                channel_type: 0,
                members: None,
                online: None,
            });
            self.create.mode = "review";
            self.create.note.clear();
            return Ok(());
        }
        let service = self.service()?;
        self.create.cancel_read();
        self.create.busy = true;
        self.create.note.clear();
        let tx = self.tx.clone();
        let generation = self.create.generation;
        let code = code.to_string();
        let text = typed.to_string();
        self.create.job = Some(self.runtime.spawn(async move {
            let result = service.invite(&code, Some(&text)).await;
            let _ = tx.send(Command::Create(Event::Reviewed(
                generation,
                result.map_err(|e| e.to_string()),
            )));
        }));
        Ok(())
    }

    fn create_group_chat(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        ensure!(
            name.chars().count() <= 100,
            "Group names can have up to 100 characters."
        );
        let friends = self.create_friends();
        let chosen = &self.create.chosen;
        ensure!(
            (2..=9).contains(&chosen.len()),
            "Choose between 2 and 9 friends."
        );
        let people = chosen
            .iter()
            .map(|id| {
                let person = friends
                    .iter()
                    .find(|p| p.id == *id)
                    .context("A selected friend is no longer available.")?;
                Ok((person.id.clone(), person.name.clone()))
            })
            .collect::<Result<Vec<_>>>()?;
        let automatic = people
            .iter()
            .map(|(_, n)| n.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if self.preview {
            self.create.sequence = self.create.sequence.wrapping_add(1);
            let id = format!("local-group-{}", self.create.sequence);
            let name = if name.is_empty() {
                automatic
            } else {
                name.into()
            };
            let detail = format!("{} members", people.len() + 1);
            self.create.groups.insert(
                0,
                Destination::new(&id, &name, &crate::view::initial(&name), &detail, false),
            );
            self.create.members.insert(id.clone(), people);
            self.samples.insert(id.clone(), vec![]);
            self.finish_create(Outcome::Group {
                id,
                name,
                note: String::new(),
            });
            return Ok(());
        }
        let service = self.service()?;
        let users = chosen.clone();
        let name = name.to_string();
        let tx = self.tx.clone();
        let generation = self.begin_create_write();
        self.create.job = Some(self.runtime.spawn(async move {
            let result = async {
                let id = service.create_group(&users).await?;
                let (name, note) = if name.is_empty() {
                    (automatic, String::new())
                } else {
                    let settings = ChannelSettings {
                        name: name.clone(),
                        ..Default::default()
                    };
                    match service.edit_channel(&id, false, &settings).await {
                        Ok(()) => (name, String::new()),
                        Err(e) => (
                            automatic,
                            format!("Group created. The name could not be saved. {e}"),
                        ),
                    }
                };
                Ok::<_, anyhow::Error>(Outcome::Group { id, name, note })
            }
            .await;
            let _ = tx.send(Command::Create(Event::Finished(
                generation,
                result.map_err(|e| e.to_string()),
            )));
        }));
        Ok(())
    }

    fn finish_create(&mut self, outcome: Outcome) {
        self.create.show("");
        self.nav.pages.clear();
        self.nav.query.clear();
        self.notice.clear();
        if let Some(service) = &self.discord {
            self.snapshot = service.snapshots().borrow().clone();
        }
        match outcome {
            Outcome::Server { id, channel, form } => {
                self.server = id;
                self.section = 1;
                self.page = Page::Channels;
                if let Some(service) = &self.discord {
                    service.view(&self.server, None);
                }
                if form {
                    self.notice="You joined the server. Complete its membership screening in Discord to post.".into();
                } else if let Some(channel) = channel.filter(|c| !c.is_empty()) {
                    let generation = self.create.generation;
                    let guild = self.server.clone();
                    let tx = self.tx.clone();
                    if let Some(service) = &self.discord {
                        let mut snapshots = service.snapshots();
                        self.create.job = Some(self.runtime.spawn(async move {
                            let visible = tokio::time::timeout(
                                std::time::Duration::from_secs(5),
                                snapshots.wait_for(|s| {
                                    s.servers
                                        .iter()
                                        .find(|g| g.id == guild)
                                        .is_some_and(|g| g.channel(&channel).is_some())
                                }),
                            )
                            .await
                            .is_ok_and(|r| r.is_ok());
                            if visible {
                                let _ = tx.send(Command::Create(Event::Landing(
                                    generation, guild, channel,
                                )));
                            }
                        }));
                    } else {
                        let _ =
                            tx.send(Command::Create(Event::Landing(generation, guild, channel)));
                    }
                }
            }
            Outcome::Group { id, name, note } => {
                self.cancel_pick();
                self.room = Room {
                    guild: PRIVATE.into(),
                    id,
                    name,
                    writable: true,
                    callable: true,
                    request: false,
                };
                self.page = Page::Chat;
                self.posts.clear();
                self.oldest = None;
                self.older = false;
                self.writing = true;
                self.collapse = true;
                self.tail += 1;
                self.notice = note;
                if let Some(service) = &self.discord {
                    service.view(PRIVATE, Some(&self.room.id));
                }
            }
        }
    }

    pub(super) fn sync_create(&mut self) {
        if self.create.open()
            && (self.create.epoch != self.epoch
                || matches!(self.page, Page::Welcome | Page::Linking))
        {
            self.create.show("");
        }
        if !std::mem::take(&mut self.create.dirty) {
            return;
        }
        let mode = self.create.mode;
        let mut name = String::new();
        let mut summary = String::new();
        let mut rows = Vec::new();
        if mode == "group" {
            let friends = self.create_friends();
            self.create
                .chosen
                .retain(|id| friends.iter().any(|f| &f.id == id));
            let query = self.create.query.trim().to_lowercase();
            summary = self
                .create
                .chosen
                .iter()
                .filter_map(|id| friends.iter().find(|p| &p.id == id))
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            for p in friends
                .into_iter()
                .filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query))
            {
                let selected = self.create.chosen.contains(&p.id);
                let image = self.pictures.get(&p.picture).and_then(Option::clone);
                rows.push((p, selected, image));
            }
        } else if mode == "review"
            && let Some(invite) = &self.create.invite
        {
            name = invite.name.clone();
            let mut fields = vec![];
            if let Some(count) = invite.online {
                fields.push(format!("{count} online"));
            }
            if let Some(count) = invite.members {
                fields.push(format!("{count} members"));
            }
            if !invite.channel_name.is_empty() {
                fields.push(format!("#{}", invite.channel_name));
            }
            summary = fields.join(" · ");
        } else if mode == "create" {
            let user = if self.preview {
                preview::LOCAL_NAME
            } else {
                self.snapshot
                    .account
                    .as_ref()
                    .map_or("", |a| a.name.as_str())
            };
            name = if user.is_empty() {
                String::new()
            } else {
                format!("{user}’s server")
            };
        }
        let selected = self.create.chosen.len() as i32;
        let note = self.create.note.clone();
        let busy = self.create.busy;
        let _ = self.ui.upgrade_in_event_loop(move |w| {
            w.set_create_name(name.into());
            w.set_create_summary(summary.into());
            w.set_create_note(note.into());
            w.set_create_busy(busy);
            w.set_create_selected(selected);
            let rows = rows
                .into_iter()
                .map(|(p, chosen, image)| crate::CreateFriend {
                    id: p.id.into(),
                    name: p.name.into(),
                    initial: p.mark.into(),
                    detail: p.detail.into(),
                    avatar: preview::picture(&p.picture)
                        .or_else(|| image.map(|p| p.image()))
                        .unwrap_or_default(),
                    chosen,
                })
                .collect::<Vec<_>>();
            w.set_create_friends(std::rc::Rc::new(slint::VecModel::from(rows)).into());
            w.set_create_mode(mode.into());
        });
    }
}
