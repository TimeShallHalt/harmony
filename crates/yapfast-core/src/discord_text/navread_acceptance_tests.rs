use super::*;
#[test]
fn acceptance_actual_finished_orders_boundary_before_disjoint_timeline_and_fences_stale() {
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
    for case in 0..4 {
        let mut channels = Channels::default();
        let mut open = OpenChannel::new("discord:@me:11").unwrap();
        open.loaded(Load::Latest, shown(page(1000..1050)));
        open.generation = 7;
        channels.open = Some(open);
        let (done, _) = tokio::sync::mpsc::unbounded_channel();
        let (events, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let load = Load::Around(1);
        let generation = if case == 1 { 6 } else { 7 };
        let result = if case == 2 {
            Err("owned boundary failure".into())
        } else {
            Ok(shown(page(if case == 3 { 2..2 } else { 2..22 })))
        };
        finished(
            Done::History {
                room: "discord:@me:11".into(),
                load,
                generation,
                result,
            },
            Some(&discord),
            &mut channels,
            &done,
            &events,
        );
        let all: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        if case == 1 {
            assert!(all.is_empty());
            assert_eq!(
                ids(channels.open.as_ref().unwrap()),
                (1000..1050).collect::<Vec<_>>()
            );
            continue;
        }
        if case == 2 {
            assert!(all.iter().any(|e| matches!(
                e,
                Event::HistoryLoaded {
                    direction: Direction::Around,
                    history: History::Failed,
                    ..
                }
            )));
            assert!(all.iter().any(|e| matches!(e, Event::RoomError { .. })));
            continue;
        }
        let h = all
            .iter()
            .position(|e| {
                matches!(
                    e,
                    Event::HistoryLoaded {
                        direction: Direction::Around,
                        history: History::Loaded { start: true, .. },
                        ..
                    }
                )
            })
            .expect("actual boundary");
        let t = all
            .iter()
            .position(|e| matches!(e, Event::Timeline { .. }))
            .expect("actual timeline");
        assert!(h < t, "production backend ordering");
        let mut app = crate::application::Application::default();
        app.state.discord_online = true;
        app.state.discord_user = Some("1".into());
        app.state.selected = Some("discord:@me:11".into());
        app.state.room_ready = true;
        let mut snapshot = yapfast_discord::service::Snapshot {
            status: Status::Online,
            user_id: Some("1".into()),
            ..Default::default()
        };
        snapshot
            .conversations
            .push(Arc::new(yapfast_discord::service::Conversation {
                id: "11".into(),
                ..Default::default()
            }));
        app.apply(Event::Discord(Arc::new(snapshot)));
        app.state.selected = Some("discord:@me:11".into());
        app.state.room_ready = true;
        // The same actual finished event stream feeds Application, not an invented order.
        app.state.messages = Arc::new(vec![Arc::new(crate::chat::ChatMessage {
            id: "1000".into(),
            item: "d:1000".into(),
            ..Default::default()
        })]);
        app.dispatch(crate::application::Action::JumpToStart);
        for e in all {
            app.apply(e);
        }
        assert!(app.state.history_start);
        if case == 3 {
            assert!(app.state.messages.is_empty());
        } else {
            assert_eq!(app.state.jump.as_ref().unwrap().0, "d:2");
        }
    }
}
