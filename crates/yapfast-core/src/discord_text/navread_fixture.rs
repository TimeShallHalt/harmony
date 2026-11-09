//! Narrow store/network-free production history completion fixture. No HTTP is issued.
use super::{open_channel::*, *};
pub fn earliest_completion() -> Vec<Event> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let discord = yapfast_discord::service::navread_fixture::publication_handle(
        yapfast_discord::service::Snapshot {
            status: Status::Online,
            user_id: Some("1".into()),
            ..Default::default()
        },
    );
    let page = |range: std::ops::Range<u64>| {
        let messages: Vec<_> = range
            .map(|i| Message {
                id: i.to_string(),
                channel_id: "11".into(),
                author_id: "1".into(),
                content: format!("owned received row {i}"),
                ..Default::default()
            })
            .collect();
        Page {
            oldest: messages.first().map(Message::order),
            full: messages.len() >= yapfast_discord::api::PAGE,
            newest: false,
            messages,
        }
    };
    let mut channels = Channels::default();
    let mut open = OpenChannel::new("discord:10:11").unwrap();
    open.loaded(Load::Latest, page(1000..1080));
    open.generation = 7;
    channels.open = Some(open);
    let (done, _) = tokio::sync::mpsc::unbounded_channel();
    let (events, mut rx) = tokio::sync::mpsc::unbounded_channel();
    commands::finished(
        Done::History {
            room: "discord:10:11".into(),
            load: Load::Around(1),
            generation: 7,
            result: Ok(page(2..42)),
        },
        Some(&discord),
        &mut channels,
        &done,
        &events,
    );
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}
