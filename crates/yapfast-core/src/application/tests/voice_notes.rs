use super::*;

fn note() -> Arc<yapfast_discord::voice_note::Note> {
    let mut encoder = yapfast_discord::voice_note::Builder::new().unwrap();
    encoder.push(&[0.25; 960]).unwrap();
    Arc::new(encoder.finish().unwrap())
}

#[test]
fn voice_note_send_is_explicit_room_bound_and_preserves_text_draft() {
    let mut app = discord_dms(vec![]);
    let room = discord::room_id(discord::DM_GUILD, "30");
    app.dispatch(Action::SelectRoom(room.clone()));
    app.apply(Event::Timeline {
        room: room.clone(),
        messages: timeline(vec![]),
    });
    app.dispatch(Action::SetDraft("keep this text".into()));
    assert!(
        app.voice_note_problem(&room).is_none(),
        "{:?}",
        app.voice_note_problem(&room)
    );
    let effects = app.dispatch(Action::SendVoiceNote {
        account: "1".into(),
        room: room.clone(),
        request: "stable-nonce".into(),
        note: note(),
    });
    assert!(
        matches!(effects.as_slice(), [Effect::Backend(Command::SendVoiceNote { request, room: to, .. })] if request == "stable-nonce" && to == &room)
    );
    assert!(
        app.dispatch(Action::SendVoiceNote {
            account: "1".into(),
            room: room.clone(),
            request: "duplicate".into(),
            note: note()
        })
        .is_empty()
    );
    app.apply(Event::VoiceNoteSent {
        request: "stale".into(),
        result: Ok(()),
    });
    assert!(
        app.state
            .voice_note_delivery
            .as_ref()
            .unwrap()
            .result
            .is_none()
    );
    app.apply(Event::VoiceNoteSent {
        request: "stable-nonce".into(),
        result: Err("service refused".into()),
    });
    assert_eq!(app.state.draft(), "keep this text");
    assert!(
        app.state
            .voice_note_delivery
            .as_ref()
            .unwrap()
            .result
            .as_ref()
            .unwrap()
            .is_err()
    );
}

#[test]
fn voice_note_respects_attach_voice_permissions_and_channel_overwrites() {
    use yapfast_discord::{
        ready::{self, permission},
        service::{Snapshot, Status},
    };
    let required = yapfast_discord::voice_note::REQUIRED_PERMISSIONS;
    for (extra, deny, allowed) in [
        (0, 0, false),
        (1 << 15, 0, false),
        (1 << 46, 0, false),
        (required, 0, true),
        (required, 1u64 << 46, false),
    ] {
        let server = ready::server(&serde_json::json!({
            "id": "10", "owner_id": "99", "properties": {"name": "Fixture"},
            "roles": [{"id":"10","name":"@everyone","permissions":(permission::VIEW_CHANNEL | permission::SEND_MESSAGES | extra).to_string()}],
            "channels":[{"id":"11","type":0,"name":"voice-notes", "permission_overwrites":[{"id":"10","type":0,"allow":"0","deny":deny.to_string()}]}]
        })).unwrap();
        let mut app = Application::default();
        app.apply(discord_event(Snapshot {
            status: Status::Online,
            user_id: Some("1".into()),
            servers: vec![marked(server, &[])],
            ..Default::default()
        }));
        let room = "discord:10:11".to_owned();
        app.dispatch(Action::Open(room.clone()));
        app.apply(Event::Timeline {
            room: room.clone(),
            messages: timeline(vec![]),
        });
        assert_eq!(
            app.voice_note_problem(&room).is_none(),
            allowed,
            "permissions {extra}, deny {deny}: {:?}",
            app.voice_note_problem(&room)
        );
    }
}

#[test]
fn voice_note_backend_shutdown_releases_pending_send() {
    for stopped in [false, true] {
        let mut app = discord_dms(vec![]);
        let room = discord::room_id(discord::DM_GUILD, "30");
        app.dispatch(Action::SelectRoom(room.clone()));
        app.apply(Event::Timeline {
            room: room.clone(),
            messages: timeline(vec![]),
        });
        let effects = app.dispatch(Action::SendVoiceNote {
            account: "1".into(),
            room,
            request: "interrupted".into(),
            note: note(),
        });
        let Effect::Backend(command) = &effects[0] else {
            panic!("expected voice command")
        };
        if stopped {
            app.stopped();
        } else {
            app.backend_failed(command);
        }
        assert!(
            app.state
                .voice_note_delivery
                .as_ref()
                .unwrap()
                .result
                .as_ref()
                .is_some_and(Result::is_err)
        );
    }
}

#[test]
fn voice_note_send_does_not_upload_in_preview_offline_or_to_a_changed_room() {
    let mut app = discord_dms(vec![]);
    let room = discord::room_id(discord::DM_GUILD, "30");
    app.dispatch(Action::SelectRoom(room.clone()));
    app.apply(Event::Timeline {
        room: room.clone(),
        messages: timeline(vec![]),
    });
    for reason in 0..4 {
        app.state.preview = reason == 0;
        app.state.discord_online = reason != 1;
        let to = if reason == 2 {
            discord::room_id(discord::DM_GUILD, "31")
        } else {
            room.clone()
        };
        assert!(
            app.dispatch(Action::SendVoiceNote {
                account: if reason == 3 { "other" } else { "1" }.into(),
                room: to,
                request: format!("blocked-{reason}"),
                note: note()
            })
            .is_empty()
        );
    }
}
