use super::*;

#[test]
fn profile_media_without_a_linked_provider_completes_with_an_explicit_error() {
    let profile = Profile {
        id: "200".into(),
        avatar: Some(
            "https://cdn.discordapp.com/avatars/200/0123456789abcdef0123456789abcdef.png?size=128"
                .into(),
        ),
        ..Default::default()
    };
    let asset = crate::profile_media::Asset::from_profile(
        "100",
        &profile,
        crate::chat::ProfilePicture::Avatar,
    )
    .unwrap();
    let (done, _) = tokio::sync::mpsc::unbounded_channel();
    let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
    command(
        Command::Discord(DiscordCommand::ViewProfileMedia {
            request: 7,
            asset,
            cancellation: crate::original::Request::new(),
        }),
        Err("offline fixture"),
        &mut Channels::default(),
        &done,
        &events,
    );
    assert!(
        matches!(received.try_recv(), Ok(Event::ProfileMedia { request: 7, loaded, .. }) if loaded.value().picture.is_err()),
        "no provider must not leave a permanent loading state"
    );
}
