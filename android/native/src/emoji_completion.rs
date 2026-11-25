use super::composer_text::{Token, token};
use super::{CATALOG, ChatEmoji, PAGE};
use crate::{
    ChatMatch, MobileWindow,
    runtime::{BackgroundRuntime, BackgroundTask},
};
use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use yapfast_discord::{
    message::Message,
    ready::{self, Access, PRIVATE, permission::USE_EXTERNAL_EMOJIS},
    service::{Discord, Snapshot},
};

pub(crate) struct Context {
    room: String,
    guild: String,
    recent: Vec<(String, String)>,
    snapshot: Arc<Snapshot>,
    service: Option<Discord>,
    runtime: Arc<BackgroundRuntime>,
    preview: bool,
}

impl Context {
    pub(crate) fn new(
        room: &str,
        guild: &str,
        messages: &[Message],
        snapshot: Arc<Snapshot>,
        service: Option<Discord>,
        runtime: Arc<BackgroundRuntime>,
        preview: bool,
    ) -> Self {
        let mut seen = HashSet::new();
        let mut recent: Vec<_> = messages
            .iter()
            .rev()
            .filter(|m| !m.author_id.is_empty() && seen.insert(m.author_id.as_str()))
            .take(64)
            .map(|m| (m.author_id.clone(), m.author.clone()))
            .collect();
        if let Some(dm) = snapshot.conversations.iter().find(|dm| dm.id == room)
            && let Some(id) = &dm.person
            && !seen.contains(id.as_str())
        {
            recent.push((id.clone(), dm.name.clone()));
        }
        Self {
            room: room.into(),
            guild: guild.into(),
            recent,
            snapshot,
            service,
            runtime,
            preview,
        }
    }

    fn same_room(&self, other: &Self) -> bool {
        self.room == other.room
            && self.guild == other.guild
            && self.preview == other.preview
            && self.snapshot.user_id == other.snapshot.user_id
    }

    fn custom(&self) -> Vec<&ready::Emoji> {
        let ours = self.snapshot.servers.iter().find(|s| s.id == self.guild);
        let nitro = self
            .snapshot
            .account
            .as_ref()
            .is_some_and(|a| a.premium_type > 0);
        let external = self.guild == PRIVATE
            || ours.is_some_and(|s| {
                s.channel(&self.room).is_some_and(|c| {
                    s.channel_permissions(
                        c.channel,
                        self.snapshot.user_id.as_deref().unwrap_or(""),
                        &s.marks.roles,
                    ) & USE_EXTERNAL_EMOJIS
                        != 0
                })
            });
        ours.into_iter()
            .chain(
                self.snapshot
                    .servers
                    .iter()
                    .filter(|s| s.id != self.guild && nitro && external),
            )
            .flat_map(|server| server.emojis.iter())
            .filter(|emoji| emoji.available && (!emoji.animated || nitro))
            .collect()
    }
}

#[derive(Default)]
struct State {
    context: Option<Context>,
    generation: u64,
    current: Option<Token>,
    remote: Vec<(String, String)>,
    search: Option<BackgroundTask<()>>,
    images: HashMap<String, Option<Image>>,
    image_order: VecDeque<String>,
    jobs: Vec<BackgroundTask<()>>,
}

thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(items)).into()
}

pub(crate) fn update(window: &MobileWindow, context: Option<Context>) {
    let (new_room, new_catalog) = STATE.with_borrow_mut(|state| {
        let new_room = match (&state.context, &context) {
            (Some(old), Some(new)) => !old.same_room(new),
            (None, None) => false,
            _ => true,
        };
        let new_catalog = new_room
            || state
                .context
                .as_ref()
                .zip(context.as_ref())
                .is_some_and(|(old, new)| !Arc::ptr_eq(&old.snapshot, &new.snapshot));
        if new_room {
            let generation = state.generation.wrapping_add(1);
            *state = State {
                generation,
                ..State::default()
            };
        }
        state.context = context;
        (new_room, new_catalog)
    });
    if new_room {
        window.set_compose_matches(Default::default());
    }
    if new_catalog {
        window.set_emoji_revision(window.get_emoji_revision().wrapping_add(1));
    }
}

fn custom_value(emoji: &ready::Emoji) -> String {
    format!(
        "<{}:{}:{}>",
        if emoji.animated { "a" } else { "" },
        emoji.name,
        emoji.id
    )
}

fn choices(state: &State, token: &Token) -> Vec<ChatMatch> {
    let Some(ctx) = &state.context else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let mut used = HashSet::new();
    let mut add = |name: &str, detail: &str, value: String, kind: &str| {
        if result.len() >= 9 || !used.insert(value.clone()) {
            return;
        }
        result.push(ChatMatch {
            name: name.into(),
            value: value.into(),
            detail: detail.into(),
            kind: kind.into(),
            start: token.start as i32,
            end: token.end as i32,
        });
    };
    let contains = |value: &str| value.to_lowercase().contains(&token.query);
    let server = ctx.snapshot.servers.iter().find(|s| s.id == ctx.guild);
    match token.prefix {
        '@' => {
            for (id, fallback) in state.remote.iter().chain(&ctx.recent) {
                let service = &ctx.service;
                let name = service
                    .as_ref()
                    .and_then(|d| d.name(&ctx.guild, id))
                    .unwrap_or_else(|| fallback.clone());
                let username = service
                    .as_ref()
                    .and_then(|d| d.username(id))
                    .unwrap_or_default();
                if contains(&name) || contains(&username) {
                    let mention = if ctx.preview {
                        format!("@{name} ")
                    } else {
                        format!("<@{id}> ")
                    };
                    add(
                        &name,
                        if username.is_empty() {
                            "Member"
                        } else {
                            &username
                        },
                        mention,
                        "user",
                    );
                }
            }
            if let Some(server) = server {
                let everyone = server
                    .channel(&ctx.room)
                    .is_some_and(|c| c.marks.mention_everyone);
                for (id, name, _) in server.mentionable(everyone) {
                    if contains(&name) {
                        add(&name, "Role", format!("<@&{id}> "), "role");
                    }
                }
                if everyone {
                    for name in ["everyone", "here"] {
                        if contains(name) {
                            add(name, "Notify members", format!("@{name} "), "role");
                        }
                    }
                }
            }
        }
        '#' => {
            if let Some(server) = server {
                for c in server.text_channels().chain(server.voice_channels()) {
                    if c.marks.access != Access::Hidden && contains(&c.channel.name) {
                        add(
                            &c.channel.name,
                            "Channel",
                            format!("<#{}> ", c.channel.id),
                            "channel",
                        );
                    }
                }
            } else if ctx.preview && ctx.guild != PRIVATE {
                for room in crate::preview::channels(&ctx.guild) {
                    if contains(&room.name) {
                        add(&room.name, "Channel", format!("#{} ", room.name), "channel");
                    }
                }
            }
        }
        ':' => {
            for emoji in ctx
                .custom()
                .into_iter()
                .filter(|e| contains(&e.name))
                .take(9)
            {
                add(
                    &format!(":{}:", emoji.name),
                    "Server emoji",
                    format!("{} ", custom_value(emoji)),
                    "emoji",
                );
            }
            for emoji in CATALOG
                .iter()
                .filter(|e| e.search.contains(&token.query.replace('_', " ")))
                .take(9)
            {
                add(
                    emoji.name,
                    emoji.symbol,
                    format!("{} ", emoji.symbol),
                    "emoji",
                );
            }
        }
        _ => {}
    }
    result
}

fn complete(window: &MobileWindow, selected: Option<Token>) {
    let updated = STATE.with_borrow_mut(|state| {
        if state.current == selected {
            return None;
        }
        state.search = None;
        state.remote.clear();
        state.current = selected.clone();
        let Some(token) = selected else {
            return Some(Vec::new());
        };
        let ctx = state.context.as_ref()?;
        let items = choices(state, &token);
        if token.prefix == '@'
            && !token.query.is_empty()
            && ctx.guild != PRIVATE
            && !ctx.guild.is_empty()
            && let Some(service) = ctx.service.clone()
        {
            let guild = ctx.guild.clone();
            let generation = state.generation;
            let weak = window.as_weak();
            state.search = Some(ctx.runtime.spawn(async move {
                tokio::time::sleep(Duration::from_millis(240)).await;
                let Ok(ids) = service.search_members(&guild, &token.query).await else {
                    return;
                };
                let people = ids
                    .into_iter()
                    .filter_map(|id| Some((id.clone(), service.name(&guild, &id)?)))
                    .collect();
                let _ = weak.upgrade_in_event_loop(move |window| {
                    let rows = STATE.with_borrow_mut(|state| {
                        if state.generation != generation || state.current.as_ref() != Some(&token)
                        {
                            return None;
                        }
                        state.remote = people;
                        Some(choices(state, &token))
                    });
                    if let Some(rows) = rows {
                        window.set_compose_matches(model(rows));
                    }
                });
            }));
        }
        Some(items)
    });
    if let Some(items) = updated {
        window.set_compose_matches(model(items));
    }
}

enum Symbol<'a> {
    Unicode(&'a super::Entry),
    Custom(&'a ready::Emoji),
}

fn page(window: &MobileWindow, query: &str, offset: i32) -> ModelRc<ChatEmoji> {
    let query = query
        .trim()
        .trim_matches(':')
        .to_lowercase()
        .replace('_', " ");
    let mut wanted = Vec::new();
    let items = STATE.with_borrow(|state| {
        let custom = state
            .context
            .as_ref()
            .map_or_else(Vec::new, Context::custom);
        let popular = if query.is_empty() { 12 } else { 0 };
        let unicode = |entry: &&super::Entry| {
            entry.symbol == query
                || query
                    .split_whitespace()
                    .all(|word| entry.search.contains(word))
        };
        CATALOG
            .iter()
            .take(popular)
            .map(Symbol::Unicode)
            .chain(
                custom
                    .into_iter()
                    .filter(|emoji| emoji.name.to_lowercase().replace('_', " ").contains(&query))
                    .map(Symbol::Custom),
            )
            .chain(
                CATALOG
                    .iter()
                    .skip(popular)
                    .filter(unicode)
                    .map(Symbol::Unicode),
            )
            .skip(offset.max(0) as usize)
            .take(PAGE)
            .map(|item| match item {
                Symbol::Unicode(entry) => ChatEmoji {
                    symbol: entry.symbol.into(),
                    key: entry.symbol.into(),
                    name: entry.name.into(),
                    ..Default::default()
                },
                Symbol::Custom(emoji) => {
                    // A still image works for both static and animated emoji in the selector.
                    let url = ready::custom_emoji(&emoji.id, false, 64);
                    let picture = state
                        .images
                        .get(&url)
                        .cloned()
                        .flatten()
                        .unwrap_or_default();
                    if !state.images.contains_key(&url) {
                        wanted.push(url);
                    }
                    ChatEmoji {
                        symbol: custom_value(emoji).into(),
                        key: format!("{}:{}", emoji.name, emoji.id).into(),
                        name: emoji.name.clone().into(),
                        picture,
                        custom: true,
                    }
                }
            })
            .collect()
    });
    if !wanted.is_empty() {
        load_images(window, wanted);
    }
    model(items)
}

fn load_images(window: &MobileWindow, wanted: Vec<String>) {
    STATE.with_borrow_mut(|state| {
        let Some(ctx) = &state.context else {
            return;
        };
        let Some(service) = ctx.service.clone() else {
            return;
        };
        let generation = state.generation;
        let weak = window.as_weak();
        for url in &wanted {
            state.images.insert(url.clone(), None);
        }
        state.jobs.retain(|job| !job.abort_handle().is_finished());
        state.jobs.push(ctx.runtime.spawn(async move {
            for batch in wanted.chunks(6) {
                let mut work = tokio::task::JoinSet::new();
                for url in batch {
                    let (service, url) = (service.clone(), url.clone());
                    work.spawn(async move {
                        let image = crate::pictures::fetch(&service, &url, false).await;
                        (url, image)
                    });
                }
                let mut ready = Vec::new();
                while let Some(Ok(row)) = work.join_next().await {
                    ready.push(row);
                }
                let _ = weak.upgrade_in_event_loop(move |window| {
                    let changed = STATE.with_borrow_mut(|state| {
                        if state.generation != generation {
                            return false;
                        }
                        let mut changed = false;
                        for (url, picture) in ready {
                            changed |= picture.is_some();
                            state.images.insert(url.clone(), picture.map(|p| p.image()));
                            state.image_order.push_back(url);
                        }
                        while state.image_order.len() > 256 {
                            if let Some(url) = state.image_order.pop_front() {
                                state.images.remove(&url);
                            }
                        }
                        changed
                    });
                    if changed {
                        window.set_emoji_revision(window.get_emoji_revision().wrapping_add(1));
                    }
                });
            }
        }));
    });
}

pub(super) fn bind(window: &MobileWindow) {
    let weak = window.as_weak();
    window.on_complete_input(move |text, anchor, caret| {
        if let Some(w) = weak.upgrade() {
            complete(&w, token(&text, anchor, caret));
        }
    });
    let weak = window.as_weak();
    window.on_emoji_results(move |query, offset| {
        weak.upgrade()
            .map_or_else(Default::default, |w| page(&w, &query, offset))
    });
}
