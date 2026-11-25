use crate::{Media, MobileWindow, Page, Place, Post, Reaction, clock, spoiler};
use slint::{Image, Model, ModelRc, SharedPixelBuffer, VecModel};
use yapfast_discord::message::{Message, Reply};

#[derive(Clone)]
pub struct Destination {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub mark: String,
    pub unread: bool,
    pub picture: String,
    pub relationship: i32,
    pub kind: String,
    pub category: String,
    pub presence: String,
    pub count: i32,
}
impl Destination {
    pub fn avatar(mut self, url: Option<&str>) -> Self {
        self.picture = url.unwrap_or_default().into();
        self
    }
    pub fn request(mut self, incoming: bool) -> Self {
        self.relationship = if incoming { 1 } else { 2 };
        self
    }
    pub fn new(id: &str, name: &str, mark: &str, detail: &str, unread: bool) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            detail: detail.into(),
            mark: mark.into(),
            unread,
            picture: String::new(),
            relationship: 0,
            kind: Default::default(),
            category: Default::default(),
            presence: Default::default(),
            count: 0,
        }
    }
}
pub fn initial(name: &str) -> String {
    name.chars().next().unwrap_or('#').to_uppercase().collect()
}
fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    std::rc::Rc::new(VecModel::from(items)).into()
}
fn sync_posts(w: &MobileWindow, rows: Vec<Post>) {
    let current = w.get_posts();
    if let Some(list) = current.as_any().downcast_ref::<VecModel<Post>>() {
        let count = list.row_count();
        if count <= rows.len()
            && rows
                .iter()
                .take(count)
                .enumerate()
                .all(|(i, p)| list.row_data(i).is_some_and(|old| old.id == p.id))
        {
            for (index, post) in rows.into_iter().enumerate() {
                if index < count {
                    list.set_row_data(index, post);
                } else {
                    list.push(post);
                }
            }
            return;
        }
    }
    w.set_posts(model(rows));
}
fn qr(address: &str) -> Image {
    let Ok(code) = qrcode::QrCode::new(address.as_bytes()) else {
        return Image::default();
    };
    let side = code.width() + 8;
    let mut pixels = SharedPixelBuffer::<slint::Rgb8Pixel>::new(side as u32, side as u32);
    for (index, pixel) in pixels.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = (index % side, index / side);
        let dark = (4..side - 4).contains(&x)
            && (4..side - 4).contains(&y)
            && code[(x - 4, y - 4)] == qrcode::Color::Dark;
        let c = if dark { 0 } else { 255 };
        *pixel = slint::Rgb8Pixel::new(c, c, c);
    }
    Image::from_rgb8(pixels)
}
#[derive(Clone, Default)]
pub(crate) struct Reveals {
    pub text: std::collections::HashSet<usize>,
    pub files: std::collections::HashSet<usize>,
}

pub struct Frame {
    pub composer_context: Option<crate::emoji::Context>,
    pub room_muted: bool,
    pub can_poll: bool,
    pub banner: String,
    pub page: Page,
    pub heading: String,
    pub name: String,
    pub handle: String,
    pub me: String,
    pub online: bool,
    pub preview: bool,
    pub notice: String,
    pub phase: i32,
    pub qr: Option<String>,
    pub scanned: String,
    pub section: i32,
    pub places: Vec<Destination>,
    pub servers: Vec<Destination>,
    pub activity: Vec<Destination>,
    pub server_id: String,
    pub room_id: String,
    pub room_name: String,
    pub room_picture: String,
    pub topic: String,
    pub direct: bool,
    pub appearance: String,
    pub requests: i32,
    pub alerts: i32,
    pub query: String,
    pub can_pin: bool,
    pub deletes: yapfast_discord::ready::Deletes,
    pub messages: Vec<Message>,
    pub draft: Option<String>,
    pub presence: String,
    pub profile: Option<(String, String, String)>,
    pub pictures: crate::pictures::Pictures,
    pub revealed: std::collections::HashMap<String, Reveals>,
    pub role_colors: std::collections::HashMap<String, u32>,
    pub authors: std::collections::HashMap<String, String>,
    pub account_picture: String,
    pub callable: bool,
    pub call_name: String,
    pub muted: bool,
    pub sharing: bool,
    pub context: String,
    pub staged: String,
    pub voice: bool,
    pub recording: bool,
    pub writable: bool,
    pub request: bool,
    pub sending: bool,
    pub older: bool,
    pub tail: i32,
    pub focus: i32,
    pub collapse: bool,
}
impl Frame {
    pub fn apply(self, w: &MobileWindow) {
        if self.page == Page::Welcome {
            crate::pictures::clear_displayed();
        }
        let image = |url: &str| {
            if self.preview && url.starts_with("preview://") {
                return crate::preview::picture(url).unwrap_or_default();
            }
            self.pictures
                .get(url)
                .and_then(Option::as_ref)
                .map_or_else(Image::default, |p| p.image())
        };
        crate::emoji::update(w, self.composer_context);
        w.set_can_poll(self.can_poll);
        w.set_room_muted(self.room_muted);
        w.set_profile_banner(image(&self.banner));
        w.set_presence(self.presence.into());
        if let Some((name, pronouns, bio)) = self.profile {
            w.set_profile_name(name.into());
            w.set_profile_pronouns(pronouns.into());
            w.set_profile_bio(bio.into());
        }
        w.set_account_picture(image(&self.account_picture));
        if w.get_page() != self.page {
            w.set_sheet("".into());
            w.set_chosen("".into());
            w.set_confirm_unlink(false);
            w.set_pending_url("".into());
            w.set_adding(false);
        }
        w.set_page(self.page);
        w.set_heading(self.heading.into());
        w.set_account_mark(initial(&self.name).into());
        w.set_account_name(self.name.into());
        w.set_account_handle(self.handle.into());
        w.set_online(self.online);
        w.set_preview(self.preview);
        w.set_callable(self.callable);
        w.set_call_name(self.call_name.into());
        w.set_call_muted(self.muted);
        w.set_sharing(self.sharing);
        w.set_notice(self.notice.into());
        w.set_phase(self.phase);
        if let Some(code) = self.qr {
            w.set_qr(qr(&code));
        } else if self.phase != 2 && w.get_qr().size().width > 0 {
            w.set_qr(Image::default());
        }
        w.set_scanned(self.scanned.into());
        w.set_section(self.section);
        let places = |items: Vec<Destination>| {
            model(
                items
                    .into_iter()
                    .map(|p| {
                        let palette = [0x6261a5, 0x387e78, 0x9d615c, 0x566ca0, 0x925c85, 0x77654c];
                        let hash =
                            p.id.bytes()
                                .fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
                        Place {
                            picture: image(&p.picture),
                            relationship: p.relationship,
                            id: p.id.into(),
                            name: p.name.into(),
                            detail: p.detail.into(),
                            mark: p.mark.into(),
                            unread: p.unread,
                            kind: p.kind.into(),
                            presence: p.presence.into(),
                            category: p.category.into(),
                            count: p.count,
                            new_category: false,
                            tint: slint::Color::from_argb_encoded(
                                0xff000000 | palette[hash % palette.len()],
                            ),
                        }
                    })
                    .collect(),
            )
        };
        w.set_servers(places(self.servers));
        w.set_activity(places(self.activity));
        w.set_places(places(self.places));
        w.set_server_id(self.server_id.into());
        w.set_selected_room(self.room_id.into());
        w.set_room_mark(initial(&self.room_name).into());
        w.set_room_name(self.room_name.into());
        w.set_room_picture(image(&self.room_picture));
        w.set_topic(self.topic.into());
        w.set_direct(self.direct);
        w.set_appearance(self.appearance.into());
        w.set_request_count(self.requests);
        w.set_alert_count(self.alerts);
        if w.get_filter_text().as_str() != self.query {
            w.set_filter_text(self.query.into());
        }
        if self.page == Page::Chat {
            let mut previous: Option<(String, u64, bool)> = None;
            sync_posts(
                w,
                self.messages
                    .into_iter()
                    .map(|m| {
                        let poll = crate::shell::polls::card(
                            &m,
                            &self.me,
                            (self.online || self.preview) && !self.request,
                        );
                        let own = m.author_id == self.me;
                        let timestamp = m.timestamp();
                        let same_day = previous
                            .as_ref()
                            .is_some_and(|(_, at, _)| clock::date(*at) == clock::date(timestamp));
                        let grouped = same_day
                            && m.reply.is_none()
                            && m.system.is_none()
                            && previous.as_ref().is_some_and(|(who, at, regular)| {
                                *regular
                                    && who == &m.author_id
                                    && timestamp.saturating_sub(*at) < 7 * 60
                            });
                        let date_label = if same_day {
                            String::new()
                        } else {
                            clock::day(timestamp)
                        };
                        previous = Some((m.author_id.clone(), timestamp, m.system.is_none()));
                        let can_delete = !m.ephemeral && self.deletes.allow(&m, &self.me);
                        let can_pin = self.can_pin && !m.ephemeral && m.system.is_none();
                        let pinned = m.pinned;
                        let mut time = clock::time(m.timestamp());
                        if m.edited {
                            time.push_str(" · edited");
                        }
                        let quote_picture = match &m.reply {
                            Some(Reply::Message { author_id, .. }) => self
                                .authors
                                .get(author_id)
                                .map_or_else(Image::default, |u| image(u)),
                            _ => Image::default(),
                        };
                        let name_color = self
                            .role_colors
                            .get(&m.author_id)
                            .map_or_else(slint::Color::default, |v| {
                                slint::Color::from_argb_encoded(0xff000000 | v)
                            });
                        let quote = match m.reply {
                            Some(Reply::Message {
                                author, content, ..
                            }) => format!(
                                "{author} · {}",
                                spoiler::hide(&content).chars().take(90).collect::<String>()
                            ),
                            Some(Reply::Deleted) => "Deleted message".into(),
                            Some(Reply::Unknown { .. }) => "Reply".into(),
                            None => String::new(),
                        };
                        let opened = self.revealed.get(&m.id);
                        let mut first = 0;
                        let mut render = |raw: &str| {
                            let linked = crate::links::linkify(&crate::links::mark_masked(raw));
                            let markdown = spoiler::mark(&linked, &m.id, first, |i| {
                                opened.is_some_and(|r| r.text.contains(&i))
                            });
                            first += spoiler::count(&linked);
                            slint::StyledText::from_markdown(&markdown).unwrap_or_else(|_| {
                                slint::StyledText::from_plain_text(&spoiler::hide(raw))
                            })
                        };
                        let raw = m.system.as_deref().unwrap_or(&m.content);
                        let body = spoiler::hide(raw);
                        let rendered = render(raw);
                        let embeds = model(
                            m.embeds
                                .iter()
                                .map(|e| {
                                    let mut body = e.description.clone().unwrap_or_default();
                                    for field in &e.fields {
                                        if !body.is_empty() {
                                            body.push_str("\n\n");
                                        }
                                        body.push_str(&format!(
                                            "**{}**\n{}",
                                            field.name, field.value
                                        ));
                                    }
                                    let content = render(&body);
                                    crate::EmbedCard {
                                        source: [e.provider.as_deref(), e.author.as_deref()]
                                            .into_iter()
                                            .flatten()
                                            .collect::<Vec<_>>()
                                            .join(" · ")
                                            .into(),
                                        title: e.title.clone().unwrap_or_default().into(),
                                        url: e.url.clone().unwrap_or_default().into(),
                                        has_content: !body.is_empty(),
                                        content,
                                        footer: e.footer.clone().unwrap_or_default().into(),
                                        accent: slint::Color::from_argb_encoded(
                                            0xff000000 | e.color.unwrap_or(0x565b79),
                                        ),
                                    }
                                })
                                .collect(),
                        );
                        Post {
                            poll,
                            embeds,
                            quote_picture,
                            name_color,
                            grouped,
                            date_label: date_label.into(),
                            can_pin,
                            can_delete,
                            pinned,
                            picture: self
                                .authors
                                .get(&m.author_id)
                                .map_or_else(Image::default, |u| image(u)),
                            rendered,
                            id: m.id.into(),
                            author: m.author.clone().into(),
                            initial: initial(&m.author).into(),
                            time: time.into(),
                            own,
                            editable: own && m.editable,
                            body: body.into(),
                            quote: quote.into(),
                            reactions: model(
                                m.reactions
                                    .into_iter()
                                    .map(|r| Reaction {
                                        label: format!("{}  {}", r.key, r.count).into(),
                                        key: r.emoji.into(),
                                        mine: r.me,
                                    })
                                    .collect(),
                            ),
                            files: model(
                                m.attachments
                                    .into_iter()
                                    .enumerate()
                                    .map(|(i, f)| Media {
                                        thumb: image(
                                            crate::pictures::source(
                                                &f,
                                                opened.is_some_and(|r| r.files.contains(&i)),
                                            )
                                            .unwrap_or(""),
                                        ),
                                        media_type: if f.spoiler
                                            && !opened.is_some_and(|r| r.files.contains(&i))
                                        {
                                            "spoiler"
                                        } else if f.shown
                                            == yapfast_discord::message::Shown::Sticker
                                        {
                                            "sticker"
                                        } else if f.video_url.is_some() {
                                            "video"
                                        } else if f.image {
                                            "image"
                                        } else if f.name.ends_with(".ogg")
                                            || f.name.ends_with(".mp3")
                                        {
                                            "audio"
                                        } else {
                                            "file"
                                        }
                                        .into(),
                                        name: f.name.into(),
                                        index: i as i32,
                                    })
                                    .collect(),
                            ),
                        }
                    })
                    .collect(),
            );
        } else {
            w.set_posts(ModelRc::default());
        }
        if let Some(draft) = self.draft {
            w.set_draft(draft.into());
        }
        w.set_context_note(self.context.into());
        w.set_staged(self.staged.into());
        w.set_voice_ready(self.voice);
        w.set_recording(self.recording);
        w.set_writable(self.writable);
        w.set_request_room(self.request);
        w.set_sending(self.sending);
        w.set_older(self.older);
        w.set_tail(self.tail);
        w.set_focus_count(self.focus);
        if self.collapse {
            w.set_chosen("".into());
        }
    }
}
