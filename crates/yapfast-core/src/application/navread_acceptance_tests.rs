use super::*;
#[test]
fn acceptance_read_all_accepted_dropped_and_late_error_expire_without_replay() {
    let mut app = Application::default();
    app.state.discord_online = true;
    app.state.discord_user = Some("1".into());
    app.state.sync_discord_reads = true;
    app.state.rooms = vec![Arc::new(ChatRoom {
        id: "discord:@me:10".into(),
        network: Network::Discord,
        unread: 1,
        last_read: Some("d:100".into()),
        ..preview::preview_room("discord:@me:10", "owned", true, 2)
    })];
    let runtime = Arc::new(crate::runtime::BackgroundRuntime::new().unwrap());
    let (backend, mut sink) = crate::backend::Backend::anonymous_fixture(runtime);
    for late_failure in [false, true] {
        app.dispatch(Action::AskReadAll);
        let effects = app.dispatch(Action::ConfirmReadAll);
        assert_eq!(effects.len(), 1);
        for e in effects {
            let Effect::Backend(c) = e else {
                panic!("only backend effect")
            };
            assert!(backend.send(c).is_ok());
        }
        assert!(
            matches!(sink.try_recv().unwrap(),Command::MarkRead{room,sync:true}if room=="discord:@me:10")
        );
        assert!(sink.try_recv().is_err());
        // Sink accepted the command, then deliberately dropped it with no acknowledgement.
        if late_failure {
            app.apply(Event::RoomError {
                room: "discord:@me:10".into(),
                message: "owned late failure".into(),
            });
        }
        assert_eq!(app.state.rooms[0].unread, 1);
        assert_eq!(app.state.rooms[0].last_read.as_deref(), Some("d:100"));
        app.dispatch(Action::AskReadAll);
        assert_eq!(app.state.read_all.as_ref().unwrap().eligible, 0);
        assert!(app.dispatch(Action::ConfirmReadAll).is_empty());
        assert!(sink.try_recv().is_err());
        for (_, sent) in app.read_all_submitted.values_mut() {
            *sent = std::time::Instant::now() - std::time::Duration::from_secs(31);
        }
        // Time passing never itself replays a command; a fresh explicit review is required.
        assert!(sink.try_recv().is_err());
        app.dispatch(Action::AskReadAll);
        assert_eq!(
            app.state.read_all.as_ref().unwrap().rooms,
            ["discord:@me:10"]
        );
        let retry = app.dispatch(Action::ConfirmReadAll);
        assert!(matches!(
            retry.as_slice(),
            [Effect::Backend(Command::MarkRead { sync: true, .. })]
        ));
        app.backend_failed(&Command::MarkRead {
            room: "discord:@me:10".into(),
            sync: true,
        });
    }
}
