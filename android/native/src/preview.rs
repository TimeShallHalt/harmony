//! Local, in-memory conversations shown only after choosing the offline preview.
use super::view::{self, Destination};
use image::ImageReader;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::{cell::OnceCell, io::Cursor, sync::OnceLock, time::SystemTime};
use yapfast_discord::message::{Attachment, Message, Reaction, Reply, Shown, snowflake_at};

pub(super) const FEATURED_SERVER: &str = "night-shift";
pub(super) const FEATURED_CHANNEL: &str = "general";
pub(super) const LOCAL_NAME: &str = "Rowan";
const SCENE: &str = "preview://afterlight";
const FILENAME: &str = "afterlight-station.webp";
const SCENE_BYTES: &[u8] = include_bytes!("../../../ui/media/preview/afterlight.webp");
const ICONS: &[u8] = include_bytes!("../../../ui/media/preview/icons.png");

struct Identity {
    id: &'static str,
    name: &'static str,
    image: &'static str,
    index: u32,
}

const fn identity(
    id: &'static str,
    name: &'static str,
    image: &'static str,
    index: u32,
) -> Identity {
    Identity {
        id,
        name,
        image,
        index,
    }
}

const IDENTITIES: &[Identity] = &[
    identity(FEATURED_SERVER, "Night Shift", "preview://night-shift", 0),
    identity("film-club", "Frame by Frame", "preview://film-club", 1),
    identity("greenhouse", "Greenhouse", "preview://greenhouse", 2),
    identity("radio-room", "Radio Room", "preview://radio-room", 3),
    identity("co-op", "Weekend co-op", "preview://co-op", 4),
    identity("mira", "Mira Vale", "preview://mira", 5),
    identity("jun", "Jun Park", "preview://jun", 6),
    identity("cleo", "Cleo", "preview://cleo", 7),
    identity("sol", "Sol", "preview://sol", 8),
    identity("alex", "Alex", "preview://alex", 9),
    identity("ada", "Ada Gray", "preview://ada", 10),
    identity("local", LOCAL_NAME, "preview://local", 11),
];

fn profile(id: &str) -> Option<&'static Identity> {
    IDENTITIES.iter().find(|p| p.id == id)
}

pub(super) fn avatar(id: &str) -> &'static str {
    profile(id).map_or("", |p| p.image)
}

fn destination(id: &str, detail: &str, unread: bool) -> Destination {
    let person = profile(id).expect("preview destination has a portrait or server icon");
    Destination::new(id, person.name, &view::initial(person.name), detail, unread)
        .avatar(Some(person.image))
}

pub(super) fn servers() -> Vec<Destination> {
    vec![
        destination(FEATURED_SERVER, "Server", true),
        destination("film-club", "Server", true),
        destination("greenhouse", "Server", false),
        destination("radio-room", "Server", false),
    ]
}

pub(super) fn dms() -> Vec<Destination> {
    vec![
        destination("mira", "Sending the clean build tonight!", true),
        destination("co-op", "Cleo: Save me a spot ✨", true),
        destination("jun", "That worked. Thank you!", false),
        destination("cleo", "The little toe beans are killing me", false),
        destination("sol", "I found the tape hiss layer.", true),
        destination("alex", "I’ll bring the spare controller.", false),
        destination("ada", "Perfect, see you at eight.", false),
    ]
}

struct Channel {
    server: &'static str,
    id: &'static str,
    name: &'static str,
    category: &'static str,
    unread: bool,
    voice: bool,
}

const fn channel(
    server: &'static str,
    id: &'static str,
    name: &'static str,
    category: &'static str,
    unread: bool,
) -> Channel {
    Channel {
        server,
        id,
        name,
        category,
        unread,
        voice: false,
    }
}

const fn voice(mut room: Channel) -> Channel {
    room.voice = true;
    room
}

const CHANNELS: &[Channel] = &[
    channel(FEATURED_SERVER, "start-here", "welcome", "Community", false),
    channel(
        FEATURED_SERVER,
        FEATURED_CHANNEL,
        "general",
        "Community",
        true,
    ),
    channel(
        FEATURED_SERVER,
        "screenshots",
        "screenshots",
        "Community",
        true,
    ),
    channel(
        FEATURED_SERVER,
        "weekend",
        "weekend-plans",
        "Community",
        false,
    ),
    channel(FEATURED_SERVER, "playtest", "playtest", "Afterlight", true),
    channel(
        FEATURED_SERVER,
        "sound-design",
        "sound-design",
        "Afterlight",
        false,
    ),
    voice(channel(
        FEATURED_SERVER,
        "after-hours",
        "After hours",
        "Voice channels",
        false,
    )),
    channel(
        "film-club",
        "screening-room",
        "screening-room",
        "Film club",
        true,
    ),
    channel("film-club", "watchlist", "watchlist", "Film club", false),
    channel(
        "greenhouse",
        "plant-chat",
        "plant-chat",
        "Greenhouse",
        false,
    ),
    channel(
        "greenhouse",
        "cuttings",
        "cutting-swap",
        "Greenhouse",
        false,
    ),
    channel("radio-room", "on-air", "on-air", "Radio Room", false),
    channel(
        "radio-room",
        "crate-digging",
        "crate-digging",
        "Radio Room",
        false,
    ),
];

pub(super) fn channels(server: &str) -> Vec<Destination> {
    CHANNELS
        .iter()
        .filter(|c| c.server == server)
        .map(|c| {
            let (mark, detail) = if c.voice {
                ("♪", "Voice channel")
            } else {
                ("#", "Text channel")
            };
            Destination::new(c.id, c.name, mark, detail, c.unread)
        })
        .collect()
}

pub(super) fn category(room: &str) -> &'static str {
    CHANNELS
        .iter()
        .find(|c| c.id == room)
        .map_or("", |c| c.category)
}

pub(super) fn channel_server(room: &str) -> Option<&'static str> {
    CHANNELS.iter().find(|c| c.id == room).map(|c| c.server)
}

pub(super) fn room_name(id: &str) -> Option<&'static str> {
    CHANNELS
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.name)
        .or_else(|| profile(id).map(|p| p.name))
}

pub(super) fn people() -> Vec<Destination> {
    vec![
        destination("mira", "Playing Afterlight", false),
        destination("jun", "Online", false),
        destination("cleo", "Listening to Radio Room", false),
        destination("sol", "Online", false),
        destination("alex", "Idle", false),
        destination("ada", "Offline", false),
    ]
}

pub(super) fn activity() -> Vec<Destination> {
    [
        (
            FEATURED_CHANNEL,
            "Mira Vale shared a screenshot",
            "Night Shift  ·  #general",
            "mira",
            true,
        ),
        (
            "mira",
            "Mira Vale sent you a message",
            "Sending the clean build tonight!",
            "mira",
            true,
        ),
        (
            "playtest",
            "Jun Park replied to you",
            "Night Shift  ·  #playtest",
            "jun",
            true,
        ),
        (
            "co-op",
            "Cleo sent a group message",
            "Weekend co-op  ·  Save me a spot ✨",
            "cleo",
            false,
        ),
    ]
    .into_iter()
    .map(|(id, name, detail, author, unread)| {
        Destination::new(id, name, &view::initial(name), detail, unread)
            .avatar(Some(avatar(author)))
    })
    .collect()
}

pub(super) fn attachment(url: &str) -> Option<(&'static str, &'static [u8])> {
    (url == SCENE).then_some((FILENAME, SCENE_BYTES))
}

fn image(rgba: image::RgbaImage) -> Image {
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    ))
}

thread_local! {
    static TILES: OnceCell<Vec<Image>> = const { OnceCell::new() };
    static LANDSCAPE: OnceCell<Option<Image>> = const { OnceCell::new() };
}

/// Called on the Slint thread. A new frame only clones already decoded images.
pub(super) fn picture(url: &str) -> Option<Image> {
    if url == SCENE {
        return LANDSCAPE.with(|scene| {
            scene
                .get_or_init(|| {
                    let original = image::load_from_memory(SCENE_BYTES).ok()?;
                    Some(image(original.thumbnail(896, 640).into_rgba8()))
                })
                .clone()
        });
    }
    let i = IDENTITIES.iter().find(|p| p.image == url)?.index as usize;
    TILES.with(|tiles| {
        tiles
            .get_or_init(|| {
                let Ok(atlas) = image::load_from_memory(ICONS) else {
                    return vec![];
                };
                let side = atlas.width() / 4;
                (0..12)
                    .map(|n| {
                        image(
                            atlas
                                .crop_imm((n % 4) * side, (n / 4) * side, side, side)
                                .into_rgba8(),
                        )
                    })
                    .collect()
            })
            .get(i)
            .cloned()
    })
}

fn railway_photo() -> Attachment {
    Attachment {
        name: FILENAME.to_owned(),
        size: Some(SCENE_BYTES.len() as u64),
        url: SCENE.into(),
        image: true,
        dimensions: ImageReader::new(Cursor::new(SCENE_BYTES))
            .with_guessed_format()
            .ok()
            .and_then(|reader| reader.into_dimensions().ok()),
        preview_url: Some(SCENE.into()),
        view_url: Some(SCENE.into()),
        still_url: None,
        video_url: None,
        link_url: None,
        animated: false,
        shown: Shown::Message,
        spoiler: false,
        description: Some("An evening railway platform in Afterlight.".into()),
    }
}

fn reaction(emoji: &str, count: usize, me: bool) -> Reaction {
    Reaction {
        key: emoji.into(),
        emoji: emoji.into(),
        me,
        count,
    }
}

pub(super) fn messages(room: &str) -> Vec<Message> {
    type Line = (u16, &'static str, &'static str);
    let lines: &[Line] = match room {
        FEATURED_CHANNEL => &[
            (
                34,
                "sol",
                "Finally got the station ambience into the build.",
            ),
            (30, "local", "How does the new platform look at dusk?"),
            (
                12,
                "mira",
                "Last train out of Afterlight. The lighting pass is finally in 🌙",
            ),
            (
                6,
                "jun",
                "That glow on the tracks! It finally feels like the last train home.",
            ),
            (
                3,
                "cleo",
                "I’d miss my stop just to stay here a little longer.",
            ),
        ],
        "start-here" => &[
            (
                2880,
                "mira",
                "Welcome to Night Shift! We’re a little group making games after work.",
            ),
            (
                2878,
                "mira",
                "Share a work in progress in #screenshots or try the latest build in #playtest.",
            ),
            (970, "cleo", "Glad I found this corner of the internet ✨"),
        ],
        "screenshots" => &[
            (
                143,
                "ada",
                "I love seeing the early versions just as much as the polished ones.",
            ),
            (
                48,
                "mira",
                "A still from the station build, before I hide the editor for the night.",
            ),
            (39, "sol", "You can almost hear the rails humming."),
        ],
        "weekend" => &[
            (175, "alex", "What are we playing this weekend?"),
            (148, "local", "Couch co-op on Saturday. My place?"),
            (141, "cleo", "I’m in. I can bring snacks!"),
        ],
        "playtest" => &[
            (
                98,
                "local",
                "Could someone try the south platform with a controller?",
            ),
            (
                42,
                "jun",
                "Tried it on my handheld. The door prompt is readable now!",
            ),
            (
                18,
                "mira",
                "Nice. That was the last fix on my list for tonight.",
            ),
        ],
        "sound-design" => &[
            (
                132,
                "sol",
                "There’s a second click right before a real station announcement. I kept it in.",
            ),
            (120, "ada", "Those half seconds of silence do so much."),
            (103, "local", "Headphones absolutely make this one."),
        ],
        "after-hours" => &[
            (
                121,
                "sol",
                "This is our usual spot for after-work playtests.",
            ),
            (90, "mira", "Dropping notes here for the next one."),
        ],
        "screening-room" => &[
            (
                182,
                "ada",
                "Friday screening starts at eight. Bring a short film for the second half.",
            ),
            (
                109,
                "jun",
                "I have a gorgeous twelve-minute stop-motion one.",
            ),
            (71, "local", "Putting the kettle on early this time."),
        ],
        "watchlist" => &[
            (301, "cleo", "Adding some quiet sci-fi to the list."),
            (
                204,
                "ada",
                "Anything with practical miniatures goes right to the top.",
            ),
            (151, "jun", "I’ll add a few after dinner."),
        ],
        "plant-chat" => &[
            (
                235,
                "cleo",
                "My pothos finally put out a leaf after two months.",
            ),
            (
                207,
                "alex",
                "The first one takes forever. Then suddenly they’re everywhere.",
            ),
            (174, "local", "Rooting for it 🌱"),
        ],
        "cuttings" => &[
            (314, "alex", "I’ve got two rooted cuttings for the swap."),
            (243, "cleo", "I can trade you a spider plant!"),
        ],
        "on-air" => &[
            (
                160,
                "sol",
                "The late show is all field recordings this week.",
            ),
            (135, "mira", "Saving it for the walk home."),
        ],
        "crate-digging" => &[
            (
                272,
                "sol",
                "Found a dusty station-jingle cassette in the dollar bin.",
            ),
            (209, "ada", "The best finds always have handwritten labels."),
        ],
        "mira" => &[
            (88, "mira", "Want another look at the platform build?"),
            (
                65,
                "local",
                "Yes please. I want to hear it with Sol’s station sounds.",
            ),
            (2, "mira", "Sending the clean build tonight!"),
        ],
        "co-op" => &[
            (84, "alex", "Saturday at six still good for everyone?"),
            (58, "local", "Yes! I’ve got a four-player save ready."),
            (37, "jun", "I’m on dinner duty."),
            (21, "cleo", "Save me a spot ✨"),
        ],
        "jun" => &[
            (
                152,
                "jun",
                "Where’s the door toggle on the handheld layout?",
            ),
            (91, "local", "Right bumper when the lamp turns amber."),
            (46, "jun", "That worked. Thank you!"),
        ],
        "cleo" => &[
            (205, "local", "Your new cat avatar is excellent."),
            (147, "cleo", "She sits on my desk every time I draw."),
            (53, "cleo", "The little toe beans are killing me"),
        ],
        "sol" => &[
            (
                192,
                "local",
                "Was the cassette sample still on your recorder?",
            ),
            (66, "sol", "I found the tape hiss layer."),
        ],
        "alex" => &[
            (200, "alex", "Hey! What are we playing this weekend?"),
            (
                124,
                "local",
                "Four-player co-op if we have enough controllers.",
            ),
            (75, "alex", "I’ll bring the spare controller."),
        ],
        "ada" => &[
            (240, "ada", "Are we keeping the early screening time?"),
            (180, "local", "Yep, eight sharp."),
            (104, "ada", "Perfect, see you at eight."),
        ],
        _ => return vec![],
    };

    // A stable clock origin keeps quotes and room selections valid for this preview session.
    static ORIGIN: OnceLock<u64> = OnceLock::new();
    let origin = *ORIGIN.get_or_init(|| {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 60
            * 60_000
    });
    let room_number = CHANNELS
        .iter()
        .position(|c| c.id == room)
        .unwrap_or_else(|| {
            CHANNELS.len()
                + IDENTITIES
                    .iter()
                    .position(|p| p.id == room)
                    .unwrap_or_default()
        }) as u64;
    let mut posts: Vec<Message> = lines
        .iter()
        .enumerate()
        .map(|(i, &(minutes, author, text))| {
            let timestamp = origin.saturating_sub(u64::from(minutes) * 60_000);
            Message {
                id: (snowflake_at(timestamp) + (room_number << 8) + i as u64).to_string(),
                channel_id: room.into(),
                author_id: author.into(),
                author: profile(author).expect("preview speaker exists").name.into(),
                content: text.into(),
                editable: true,
                ..Message::default()
            }
        })
        .collect();
    match room {
        FEATURED_CHANNEL => {
            posts[2].attachments.push(railway_photo());
            posts[2].reactions = vec![reaction("🔥", 8, true), reaction("💜", 5, false)];
            posts[3].reply = Some(Reply::quoting(&posts[2]));
            posts[4].reactions.push(reaction("🌙", 3, false));
        }
        "screenshots" => {
            posts[1].attachments.push(railway_photo());
            posts[1].reactions.push(reaction("✨", 5, false));
        }
        "playtest" | "cuttings" | "on-air" | "crate-digging" => {
            posts[1].reply = Some(Reply::quoting(&posts[0]));
            posts[1].reactions.push(reaction("🙌", 2, false));
        }
        "weekend" | "co-op" | "plant-chat" | "screening-room" => {
            posts[1].reactions.push(reaction("💚", 3, true));
        }
        "mira" | "jun" | "alex" | "ada" | "sound-design" | "start-here" => {
            posts[2].reply = Some(Reply::quoting(&posts[1]));
        }
        _ => {}
    }
    posts
}
