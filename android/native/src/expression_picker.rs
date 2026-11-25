use super::{Command, Page, Sender, Shell};
use crate::{
    pictures::{self, Pictures},
    runtime::BackgroundTask,
};
use anyhow::{Result, ensure};
use slint::{ModelRc, VecModel};
use std::{
    collections::HashSet,
    sync::{Arc, Weak},
    time::Duration,
};
use yapfast_discord::{
    gifs::Gif,
    ready::{PRIVATE, permission},
    service::{Discord, Snapshot},
    stickers::Sticker,
};

const PAGE_SIZE: usize = 16;

#[derive(Clone)]
pub(crate) struct Choice {
    value: String,
    name: String,
    group: String,
    image: String,
    server: Option<String>,
    provider_id: Option<String>,
}

impl Choice {
    fn gif(gif: Gif) -> Option<Self> {
        if !gif.url.starts_with("https://") {
            return None;
        }
        let slug = gif
            .url
            .split(['?', '#'])
            .next()?
            .trim_end_matches('/')
            .rsplit('/')
            .next()?;
        let name = slug
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .trim_end_matches('-')
            .trim_end_matches("-gif")
            .replace(['-', '_'], " ");
        Some(Self {
            value: gif.url,
            name: if name.is_empty() {
                "GIF".into()
            } else {
                name.chars().take(72).collect()
            },
            group: String::new(),
            image: gif.src,
            server: None,
            provider_id: gif.id,
        })
    }

    fn sticker(sticker: &Sticker, group: &str, server: Option<&str>) -> Option<Self> {
        (!sticker.unavailable).then(|| Self {
            value: sticker.id.clone(),
            name: sticker.name.clone(),
            group: group.into(),
            image: sticker.picture(),
            server: server.map(str::to_owned),
            provider_id: None,
        })
    }
}

pub(crate) enum Event {
    Input(String, String),
    Catalog(u64, Vec<Choice>, String),
    Thumbnails(u64, Pictures),
    StickerSent(StickerSend, Result<(), String>),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct StickerSend {
    epoch: u64,
    room: String,
    generation: u64,
}

#[derive(Default)]
pub(super) struct Picker {
    mode: &'static str,
    query: String,
    room: String,
    epoch: u64,
    page: usize,
    choices: Vec<Choice>,
    pictures: Pictures,
    note: String,
    busy: bool,
    dirty: bool,
    writable: bool,
    snapshot: Option<Weak<Snapshot>>,
    pending: Option<StickerSend>,
    generation: u64,
    image_generation: u64,
    load: Option<BackgroundTask<()>>,
    images: Option<BackgroundTask<()>>,
}

impl Picker {
    pub(super) fn open(&self) -> bool {
        !self.mode.is_empty()
    }

    fn stop(&mut self) {
        self.load.take();
        self.images.take();
        self.generation = self.generation.wrapping_add(1);
        self.image_generation = self.image_generation.wrapping_add(1);
    }

    fn matches(&self) -> Vec<&Choice> {
        let query = self.query.trim().to_lowercase();
        self.choices
            .iter()
            .filter(|item| {
                self.mode != "stickers"
                    || query.is_empty()
                    || item.name.to_lowercase().contains(&query)
                    || item.group.to_lowercase().contains(&query)
            })
            .collect()
    }
}

pub(super) fn bind(window: &crate::MobileWindow, tx: Sender) {
    window.on_expression(move |command, value| {
        let _ = tx.send(Command::Expression(Event::Input(
            command.into(),
            value.into(),
        )));
    });
}

impl Shell {
    pub(super) fn close_expressions(&mut self) {
        self.expressions.stop();
        self.expressions.mode = "";
        self.expressions.choices.clear();
        self.expressions.pictures.clear();
        self.expressions.note.clear();
        self.expressions.dirty = true;
    }

    fn can_send_expression(&self) -> bool {
        self.page == Page::Chat
            && !self.preview
            && self.online()
            && self.room.writable
            && !self.room.request
            && self.discord.is_some()
    }

    fn can_use_sticker(&self, item: &Choice) -> bool {
        let Some(source) = item.server.as_deref() else {
            return true;
        };
        let Some(owner) = self.snapshot.servers.iter().find(|s| s.id == source) else {
            return false;
        };
        if owner
            .stickers
            .iter()
            .find(|s| s.id == item.value)
            .is_some_and(|s| s.unavailable)
        {
            return false;
        }
        if source == self.room.guild {
            return true;
        }
        if !self
            .snapshot
            .account
            .as_ref()
            .is_some_and(|account| account.premium_type > 0)
        {
            return false;
        }
        if self.room.guild == PRIVATE {
            return true;
        }
        self.snapshot
            .servers
            .iter()
            .find(|s| s.id == self.room.guild)
            .is_some_and(|s| {
                s.channel(&self.room.id).is_some_and(|c| {
                    s.roles.is_empty()
                        || s.channel_permissions(c.channel, self.me(), &s.marks.roles)
                            & permission::USE_EXTERNAL_STICKERS
                            != 0
                })
            })
    }

    pub(super) fn sync_expressions(&mut self) {
        if self
            .expressions
            .pending
            .as_ref()
            .is_some_and(|request| request.epoch != self.epoch)
        {
            self.expressions.pending = None;
        }
        if self.expressions.open()
            && (self.page != Page::Chat
                || self.expressions.epoch != self.epoch
                || self.expressions.room != self.room.id)
        {
            self.close_expressions();
        }
        if self.expressions.open() {
            if self.expressions.writable != self.can_send_expression()
                && let Err(error) = self.load_expressions(false)
            {
                self.expressions.note = error.to_string();
                self.expressions.dirty = true;
            }
            if self
                .expressions
                .snapshot
                .as_ref()
                .and_then(Weak::upgrade)
                .is_none_or(|previous| !Arc::ptr_eq(&previous, &self.snapshot))
            {
                self.expressions.snapshot = Some(Arc::downgrade(&self.snapshot));
                self.expressions.dirty = true;
            }
        }
        if !std::mem::take(&mut self.expressions.dirty) {
            return;
        }
        let state = &self.expressions;
        let mode = state.mode;
        let choices = state.matches();
        let start = state.page * PAGE_SIZE;
        let more = start + PAGE_SIZE < choices.len();
        let page = state.page as i32;
        let sending = state.pending.is_some() && mode == "stickers";
        let writable = self.can_send_expression() && !sending;
        let mut rows = Vec::new();
        for choice in choices.into_iter().skip(start).take(PAGE_SIZE) {
            rows.push((
                choice.value.clone(),
                choice.name.clone(),
                choice.group.clone(),
                state.pictures.get(&choice.image).and_then(Option::clone),
                writable && (mode != "stickers" || self.can_use_sticker(choice)),
            ));
        }
        let query = state.query.clone();
        let busy = state.busy || sending;
        let note = if sending {
            "Sending sticker…".to_owned()
        } else {
            state.note.clone()
        };
        let _ = self.ui.upgrade_in_event_loop(move |w| {
            let mut grid = Vec::new();
            let mut cells = rows.into_iter();
            while let Some((id, label, group, thumb, usable)) = cells.next() {
                let mut row = crate::ExpressionRow {
                    left_id: id.into(),
                    left_name: label.into(),
                    left_group: group.into(),
                    left_image: thumb.map_or_else(Default::default, |p| p.image()),
                    left_usable: usable,
                    ..Default::default()
                };
                if let Some((id, label, group, thumb, usable)) = cells.next() {
                    row.right_id = id.into();
                    row.right_name = label.into();
                    row.right_group = group.into();
                    row.right_image = thumb.map_or_else(Default::default, |p| p.image());
                    row.right_usable = usable;
                }
                grid.push(row);
            }
            w.set_expression_mode(mode.into());
            w.set_expression_query(query.into());
            w.set_expression_note(note.into());
            w.set_expression_busy(busy);
            w.set_expression_page(page);
            w.set_expression_more(more);
            w.set_expression_rows(ModelRc::new(VecModel::from(grid)));
        });
    }

    fn expression_thumbnails(&mut self) {
        let state = &mut self.expressions;
        state.dirty = true;
        state.images.take();
        state.image_generation = state.image_generation.wrapping_add(1);
        let urls: HashSet<_> = state
            .matches()
            .into_iter()
            .skip(state.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .map(|c| c.image.clone())
            .filter(|url| !url.is_empty() && !yapfast_discord::ready::lottie_sticker(url))
            .collect();
        state.pictures.retain(|url, _| urls.contains(url));
        let missing: Vec<_> = urls
            .into_iter()
            .filter(|url| !state.pictures.contains_key(url))
            .collect();
        let Some(discord) = self.discord.clone() else {
            return;
        };
        if missing.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let generation = state.image_generation;
        state.images = Some(self.runtime.spawn(async move {
            for group in missing.chunks(4) {
                let mut batch = tokio::task::JoinSet::new();
                for url in group.iter().cloned() {
                    let discord = discord.clone();
                    batch.spawn(async move {
                        let preview = pictures::fetch(&discord, &url, true).await;
                        (url, preview)
                    });
                }
                let mut ready = Pictures::new();
                while let Some(result) = batch.join_next().await {
                    if let Ok((url, image)) = result {
                        ready.insert(url, image);
                    }
                }
                if tx
                    .send(Command::Expression(Event::Thumbnails(generation, ready)))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn load_expressions(&mut self, debounce: bool) -> Result<()> {
        self.expressions.stop();
        self.expressions.choices.clear();
        self.expressions.pictures.clear();
        self.expressions.note.clear();
        self.expressions.page = 0;
        self.expressions.busy = false;
        self.expressions.dirty = true;
        self.expressions.writable = self.can_send_expression();
        if !self.expressions.writable {
            self.expressions.note = if self.preview {
                "Link an account to browse and send expressions."
            } else {
                "Open a writable conversation while connected to Discord."
            }
            .into();
            return Ok(());
        }
        let discord = self.service()?;
        let generation = self.expressions.generation;
        let tx = self.tx.clone();
        let query = self.expressions.query.trim().to_owned();
        let mode = self.expressions.mode;
        let current = self
            .snapshot
            .servers
            .iter()
            .find(|s| s.id == self.room.guild);
        let refresh = current
            .filter(|s| s.stickers.is_empty())
            .map(|s| (s.id.clone(), s.name.clone()));
        if mode == "stickers" {
            let mut servers: Vec<_> = self.snapshot.servers.iter().collect();
            servers.sort_by_key(|s| (s.id != self.room.guild, s.name.to_lowercase()));
            for server in servers {
                self.expressions
                    .choices
                    .extend(server.stickers.iter().filter_map(|sticker| {
                        Choice::sticker(sticker, &server.name, Some(&server.id))
                    }));
            }
            self.expression_thumbnails();
        }
        self.expressions.busy = true;
        self.expressions.load = Some(self.runtime.spawn(async move {
            if debounce {
                tokio::time::sleep(Duration::from_millis(240)).await;
            }
            let (choices, note) = if mode == "stickers" {
                load_stickers(&discord, refresh).await
            } else {
                let response = if query.is_empty() {
                    discord.gifs_trending_gifs().await
                } else {
                    discord.gifs_search(&query).await
                };
                match response {
                    Ok(gifs) => (
                        gifs.into_iter().filter_map(Choice::gif).collect(),
                        String::new(),
                    ),
                    Err(err) => (vec![], format!("Couldn’t load GIFs. {err}")),
                }
            };
            let _ = tx.send(Command::Expression(Event::Catalog(
                generation, choices, note,
            )));
        }));
        Ok(())
    }

    pub(super) async fn expression_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::StickerSent(request, result) => {
                if self.expressions.pending.as_ref() != Some(&request) {
                    return Ok(());
                }
                self.expressions.pending = None;
                self.expressions.dirty = true;
                if request.epoch == self.epoch
                    && self.page == Page::Chat
                    && request.room == self.room.id
                {
                    let same_tray = self.expressions.mode == "stickers"
                        && self.expressions.generation == request.generation;
                    match result {
                        Ok(()) if same_tray => self.close_expressions(),
                        Err(error) if same_tray => self.expressions.note = error,
                        Err(error) => self.notice = error,
                        Ok(()) => {}
                    }
                }
            }
            Event::Input(command, _) if command == "close" => self.close_expressions(),
            Event::Input(command, _) if command == "gifs" || command == "stickers" => {
                if self.page != Page::Chat {
                    return Ok(());
                }
                let mode = if command == "gifs" {
                    "gifs"
                } else {
                    "stickers"
                };
                if self.expressions.mode == mode {
                    return Ok(());
                }
                self.expressions.room = self.room.id.clone();
                self.expressions.epoch = self.epoch;
                self.expressions.mode = mode;
                self.expressions.query.clear();
                self.load_expressions(false)?;
            }
            _ if !self.expressions.open()
                || self.page != Page::Chat
                || self.expressions.room != self.room.id
                || self.expressions.epoch != self.epoch => {}
            Event::Catalog(generation, items, note)
                if generation == self.expressions.generation =>
            {
                self.expressions.busy = false;
                self.expressions.note = note;
                let mut seen: HashSet<_> = self
                    .expressions
                    .choices
                    .iter()
                    .map(|c| c.value.clone())
                    .collect();
                self.expressions
                    .choices
                    .extend(items.into_iter().filter(|c| seen.insert(c.value.clone())));
                if self.expressions.mode == "stickers" {
                    self.expressions
                        .choices
                        .sort_by_key(|c| match c.server.as_deref() {
                            Some(server) if server == self.room.guild => 0,
                            Some(_) => 1,
                            None => 2,
                        });
                }
                self.expression_thumbnails();
            }
            Event::Thumbnails(generation, pictures)
                if generation == self.expressions.image_generation =>
            {
                self.expressions.pictures.extend(pictures);
                self.expressions.dirty = true;
            }
            Event::Input(command, value) => match command.as_str() {
                "search" => {
                    let query: String = value.chars().take(160).collect();
                    if self.expressions.query != query {
                        self.expressions.query = query;
                        self.expressions.page = 0;
                        if self.expressions.mode == "gifs" {
                            self.load_expressions(true)?;
                        } else {
                            self.expression_thumbnails();
                        }
                    }
                }
                "retry" => self.load_expressions(false)?,
                "next"
                    if (self.expressions.page + 1) * PAGE_SIZE
                        < self.expressions.matches().len() =>
                {
                    self.expressions.page += 1;
                    self.expression_thumbnails();
                }
                "previous" if self.expressions.page > 0 => {
                    self.expressions.page -= 1;
                    self.expression_thumbnails();
                }
                "pick" => {
                    let selected = self
                        .expressions
                        .matches()
                        .into_iter()
                        .skip(self.expressions.page * PAGE_SIZE)
                        .take(PAGE_SIZE)
                        .find(|choice| choice.value == value)
                        .cloned();
                    if let Some(selected) = selected
                        && let Err(err) = self.pick_expression(selected).await
                    {
                        self.expressions.busy = false;
                        self.expressions.note = err.to_string();
                        self.expressions.dirty = true;
                        if !self.expressions.open() {
                            self.notice = err.to_string();
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    async fn pick_expression(&mut self, item: Choice) -> Result<()> {
        ensure!(
            self.can_send_expression(),
            "This conversation is not writable."
        );
        let service = self.service()?;
        if self.expressions.mode == "stickers" {
            ensure!(
                self.can_use_sticker(&item),
                "You cannot use that sticker in this channel."
            );
            ensure!(
                self.compose().edit.is_none(),
                "Finish editing before sending a sticker."
            );
            if self.expressions.pending.is_some() {
                return Ok(());
            }
            let request = StickerSend {
                epoch: self.epoch,
                room: self.room.id.clone(),
                generation: self.expressions.generation,
            };
            self.expressions.pending = Some(request.clone());
            self.expressions.dirty = true;
            let guild = self.room.guild.clone();
            let tx = self.tx.clone();
            // The tray can close while the acknowledged send finishes for its original room.
            tokio::spawn(async move {
                let result = service
                    .send_sticker(&guild, &request.room, &item.value)
                    .await
                    .map_err(|error| format!("Couldn’t send this sticker. {error}"));
                let _ = tx.send(Command::Expression(Event::StickerSent(request, result)));
            });
            return Ok(());
        }
        ensure!(
            self.compose().note.is_none(),
            "Send or remove the voice message before adding a GIF."
        );
        let query = self.expressions.query.trim().to_owned();
        if let Some(id) = item.provider_id
            && !query.is_empty()
        {
            tokio::spawn(async move {
                let _ = service.gif_select(&id, &query).await;
            });
        }
        let composer = self.compose();
        let has_draft =
            !composer.body.is_empty() || composer.edit.is_some() || !composer.files.is_empty();
        if !composer.body.is_empty() && !composer.body.ends_with(char::is_whitespace) {
            composer.body.push(' ');
        }
        composer.body.push_str(&item.value);
        composer.nonce = None;
        self.writing = true;
        self.close_expressions();
        if !has_draft {
            self.send().await?;
        }
        Ok(())
    }
}

async fn load_stickers(
    discord: &Discord,
    current: Option<(String, String)>,
) -> (Vec<Choice>, String) {
    let own = async {
        let Some((server, name)) = current else {
            return Ok(vec![]);
        };
        let stickers = discord.guild_stickers(&server).await?;
        anyhow::Ok(
            stickers
                .into_iter()
                .filter_map(|s| {
                    Choice::sticker(
                        &Sticker {
                            id: s.id,
                            name: s.name,
                            format: u64::from(s.format),
                            unavailable: !s.available,
                        },
                        &name,
                        Some(&server),
                    )
                })
                .collect(),
        )
    };
    let (own, packs) = tokio::join!(own, discord.sticker_packs());
    let mut all = Vec::new();
    let mut failures = Vec::new();
    match own {
        Ok(choices) => all.extend(choices),
        Err(error) => failures.push(format!("Couldn’t load this server’s stickers. {error}")),
    }
    match packs {
        Ok(packs) => {
            for pack in packs {
                all.extend(
                    pack.stickers
                        .iter()
                        .filter_map(|s| Choice::sticker(s, &pack.name, None)),
                );
            }
        }
        Err(error) => failures.push(format!("Couldn’t load sticker packs. {error}")),
    }
    (all, failures.join(" "))
}
