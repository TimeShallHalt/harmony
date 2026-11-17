use super::*;
use yapfast_core::chat::DiscordCommand;

#[derive(Debug)]
pub(super) struct NativeNameGlyph {
    pub kind: &'static str,
    pub family: String,
    pub size: f32,
    pub brush: slint::Brush,
}

/// Inspect the actual rendered item, not the profile's projected font fields.
/// people.slint deliberately uses a selectable, read-only TextInput for plain
/// names and Text for styled names. Slint can lower Text to either native type.
/// HasFont resolves inherited Window defaults just as the renderer does; a
/// plain TextInput/SimpleText need not have a local font-family property.
pub(super) fn native_name_glyph(
    item: &i_slint_core::item_tree::ItemRc,
    name: &str,
) -> Option<NativeNameGlyph> {
    use i_slint_core::item_rendering::HasFont;
    macro_rules! inspect {
        ($ty:ident) => {
            if let Some(text) = item.downcast::<i_slint_core::items::$ty>() {
                let text = text.as_pin_ref();
                if i_slint_core::items::$ty::FIELD_OFFSETS
                    .text()
                    .apply_pin(text)
                    .get()
                    == name
                {
                    let font = text.font_request(item);
                    return Some(NativeNameGlyph {
                        kind: stringify!($ty),
                        family: font.family.unwrap_or_default().to_string(),
                        size: font.pixel_size.unwrap_or_default().0,
                        brush: i_slint_core::items::$ty::FIELD_OFFSETS
                            .color()
                            .apply_pin(text)
                            .get(),
                    });
                }
            }
        };
    }
    inspect!(TextInput);
    inspect!(SimpleText);
    inspect!(ComplexText);
    None
}

pub(super) fn repair1_profile_command(effects: Vec<Effect>) -> Command {
    effects
        .into_iter()
        .find_map(|effect| {
            if let Effect::Backend(command @ Command::Discord(DiscordCommand::ScopedProfile(_))) =
                effect
            {
                Some(command)
            } else {
                None
            }
        })
        .expect("real scoped profile request; reset any already-open panel before requesting again")
}
pub(super) fn repair1_profile_response(
    command: Command,
    kept: bool,
    result: Result<yapfast_core::chat::Profile, String>,
) -> Event {
    match command {
        Command::Discord(DiscordCommand::ScopedProfile(request)) => {
            if request.publication.lock().unwrap().is_none() {
                let snapshot = yapfast_discord::service::Snapshot {
                    status: yapfast_discord::service::Status::Online,
                    user_id: request.owner.clone(),
                    presence_observation: request.source.clone(),
                    ..Default::default()
                };
                let (publication, _updates) =
                    yapfast_core::chat::ProfilePublication::fixture(Arc::new(snapshot));
                *request.publication.lock().unwrap() = Some(publication);
            }
            Event::ProfilePresentation {
                request,
                kept,
                result,
            }
        }
        _ => panic!("expected a real scoped profile request"),
    }
}

#[test]
fn repair1_delayed_profile_context_and_reopen_do_not_fill_new_request() {
    i_slint_backend_testing::init_no_event_loop();
    for (kept, error, same_context) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
        (true, false, true),
        (false, true, true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let window = YapfastWindow::new().unwrap();
        let owner = Adapter::new(
            Arc::new(BackgroundRuntime::new().unwrap()),
            dir.path().into(),
            &window,
        )
        .unwrap();
        let mut a = owner.borrow_mut();
        let mut snapshot = yapfast_discord::service::Snapshot {
            status: yapfast_discord::service::Status::Online,
            user_id: Some("100".into()),
            ..Default::default()
        };
        snapshot
            .conversations
            .push(Arc::new(yapfast_discord::service::Conversation {
                id: "10".into(),
                name: "Ada".into(),
                person: Some("200".into()),
                ..Default::default()
            }));
        a.apply(Event::Discord(Arc::new(snapshot)));
        let mut rooms = a.app.state.rooms.to_vec();
        let mut guild = rooms
            .iter()
            .find(|room| room.id == "discord:@me:10")
            .unwrap()
            .clone();
        Arc::make_mut(&mut guild).id = "discord:20:11".into();
        rooms.push(guild);
        a.app.state.rooms = rooms.into();
        a.app.state.selected = Some("discord:@me:10".into());
        let old = repair1_profile_command(
            a.app
                .dispatch(Action::ShowDmProfile("discord:@me:10".into())),
        );
        a.app.dispatch(Action::CloseProfile);
        let current = repair1_profile_command(
            a.app.dispatch(Action::ShowDmProfile(
                if same_context {
                    "discord:@me:10"
                } else {
                    "discord:20:11"
                }
                .into(),
            )),
        );
        let styled = yapfast_core::chat::Profile {
            id: "200".into(),
            name: "wrong context".into(),
            name_style: Some(yapfast_core::appearance::NameStyle {
                font_id: 1,
                effect_id: 8,
                colors: vec![0xff0000],
            }),
            ..Default::default()
        };
        a.apply(repair1_profile_response(
            old,
            kept,
            if error {
                Err("old context failed".into())
            } else {
                Ok(styled.clone())
            },
        ));
        let view = a.app.state.profile.as_ref().unwrap();
        assert!(
            view.loading,
            "old context/closed request must not complete reopened loading"
        );
        assert!(
            view.profile.name_style.is_none(),
            "old appearance must not install in new context"
        );
        a.apply(repair1_profile_response(
            current,
            false,
            Ok(yapfast_core::chat::Profile {
                name: "correct context".into(),
                ..styled
            }),
        ));
        let view = a.app.state.profile.as_ref().unwrap();
        assert!(!view.loading);
        assert_eq!(view.profile.name, "correct context");
    }
}

#[test]
fn repair1_profile_publication_rejects_live_provider_change_before_application_snapshot() {
    i_slint_backend_testing::init_no_event_loop();
    for outcome in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let window = YapfastWindow::new().unwrap();
        let owner = Adapter::new(
            Arc::new(BackgroundRuntime::new().unwrap()),
            dir.path().into(),
            &window,
        )
        .unwrap();
        let mut a = owner.borrow_mut();
        let mut snapshot = yapfast_discord::service::Snapshot {
            status: yapfast_discord::service::Status::Online,
            user_id: Some("100".into()),
            ..Default::default()
        };
        snapshot
            .conversations
            .push(Arc::new(yapfast_discord::service::Conversation {
                id: "10".into(),
                name: "Ada".into(),
                person: Some("200".into()),
                ..Default::default()
            }));
        let snapshot = Arc::new(snapshot);
        a.apply(Event::Discord(snapshot.clone()));
        a.app.state.selected = Some("discord:@me:10".into());
        let command = repair1_profile_command(
            a.app
                .dispatch(Action::ShowDmProfile("discord:@me:10".into())),
        );
        let Command::Discord(DiscordCommand::ScopedProfile(request)) = &command else {
            panic!("scoped production request")
        };
        let (publication, live) = yapfast_core::chat::ProfilePublication::fixture(snapshot.clone());
        *request.publication.lock().unwrap() = Some(publication.clone());
        // The real provider has changed, while Application still holds A.
        let mut changed = (*snapshot).clone();
        changed.user_id = Some("101".into());
        live(Arc::new(changed));
        let result = if outcome == 2 {
            Err("old failure".into())
        } else {
            Ok(yapfast_core::chat::Profile {
                id: "200".into(),
                name: "old provider".into(),
                ..Default::default()
            })
        };
        a.apply(repair1_profile_response(
            command.clone(),
            outcome == 1,
            result,
        ));
        assert!(
            a.app.state.profile.as_ref().unwrap().loading,
            "live provider replacement must fence queued profile completion before a new Application snapshot"
        );
        publication.retire();
        live(snapshot);
        a.apply(repair1_profile_response(
            command,
            false,
            Err("late failure after provider A-B-A".into()),
        ));
        assert!(
            a.app.state.profile.as_ref().unwrap().loading,
            "retired provider authority must not revive on A-B-A"
        );
    }
}

#[test]
fn repair1_concurrent_panel_and_card_keep_distinct_profile_contexts() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    let mut a = owner.borrow_mut();
    let mut snapshot = yapfast_discord::service::Snapshot {
        status: yapfast_discord::service::Status::Online,
        user_id: Some("100".into()),
        ..Default::default()
    };
    snapshot
        .conversations
        .push(Arc::new(yapfast_discord::service::Conversation {
            id: "10".into(),
            name: "Ada".into(),
            person: Some("200".into()),
            ..Default::default()
        }));
    a.apply(Event::Discord(Arc::new(snapshot)));
    a.app.state.selected = Some("discord:@me:10".into());
    a.app.dispatch(Action::ShowProfilePanel(false));
    let panel = repair1_profile_command(a.app.dispatch(Action::ShowProfilePanel(true)));
    let mut rooms = a.app.state.rooms.to_vec();
    let mut guild = rooms
        .iter()
        .find(|room| room.id == "discord:@me:10")
        .unwrap()
        .clone();
    Arc::make_mut(&mut guild).id = "discord:20:11".into();
    rooms.push(guild);
    a.app.state.rooms = rooms.into();
    let card = repair1_profile_command(
        a.app
            .dispatch(Action::ShowDmProfile("discord:20:11".into())),
    );
    let profile = |font| yapfast_core::chat::Profile {
        id: "200".into(),
        name: "Ada".into(),
        name_style: Some(yapfast_core::appearance::NameStyle {
            font_id: font,
            effect_id: 0,
            colors: vec![0xff0000],
        }),
        ..Default::default()
    };
    a.apply(repair1_profile_response(panel, false, Ok(profile(1))));
    assert!(!a.app.state.panel_profile.as_ref().unwrap().loading);
    assert!(
        a.app.state.profile.as_ref().unwrap().loading,
        "global panel completion must not complete a simultaneous guild card"
    );
    a.apply(repair1_profile_response(card, true, Ok(profile(2))));
    assert_eq!(
        a.app
            .state
            .panel_profile
            .as_ref()
            .unwrap()
            .profile
            .name_style
            .as_ref()
            .unwrap()
            .font_id,
        1
    );
    assert_eq!(
        a.app
            .state
            .profile
            .as_ref()
            .unwrap()
            .profile
            .name_style
            .as_ref()
            .unwrap()
            .font_id,
        2
    );
    assert!(a.app.state.profile.as_ref().unwrap().loading);
}

#[test]
fn received_style_projects_into_real_profile_card_full_and_panel() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let adapter = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    adapter.borrow_mut().layout.sound_volume = Some(0);
    bind_callbacks(&window, &adapter);
    let style = yapfast_discord::profile::parse(&serde_json::json!({"user":{"id":"200","username":"Ada","display_name_styles":{"font_id":1,"effect_id":8,"colors":[16711680,255]}}}), None).unwrap().name_style;
    let profile = yapfast_core::chat::Profile {
        id: "200".into(),
        name: "Ada".into(),
        name_style: style.map(Into::into),
        ..Default::default()
    };
    let view = ProfileView {
        profile,
        large: false,
        loading: false,
        kept: false,
        note: None,
        platform_presence: None,
        board_open: false,
        activity_open: false,
        wishlist_open: false,
    };
    let (backend, _commands) = Backend::anonymous_fixture(adapter.borrow().runtime.clone());
    let mut a = adapter.borrow_mut();
    a.backend = Some(backend);
    let mut snapshot = yapfast_discord::service::Snapshot {
        status: yapfast_discord::service::Status::Online,
        user_id: Some("100".into()),
        ..Default::default()
    };
    snapshot
        .conversations
        .push(Arc::new(yapfast_discord::service::Conversation {
            id: "10".into(),
            name: "Ada".into(),
            person: Some("200".into()),
            ..Default::default()
        }));
    a.apply(Event::Discord(Arc::new(snapshot)));
    a.app.state.account = Some("100".into());
    a.app.state.trusted = true;
    a.app.state.navigation = yapfast_core::application::Navigation::DirectMessages;
    a.app.state.selected = Some("discord:@me:10".into());
    a.app.state.dm_screen = None;
    // Applying the snapshot may already have opened the default DM panel.
    // Reopening it creates a fresh request rather than relying on a no-op.
    a.app.dispatch(Action::ShowProfilePanel(false));
    let panel = repair1_profile_command(a.app.dispatch(Action::ShowProfilePanel(true)));
    let card = repair1_profile_command(a.app.dispatch(Action::ShowProfile {
        user: "200".into(),
        large: false,
    }));
    if let (
        Command::Discord(DiscordCommand::ScopedProfile(panel)),
        Command::Discord(DiscordCommand::ScopedProfile(card)),
    ) = (&panel, &card)
    {
        assert!(panel.panel);
        assert!(!card.panel);
        assert_eq!(panel.room, "discord:@me:10");
        assert_eq!(card.room, "discord:@me:10");
        assert!(!Arc::ptr_eq(panel, card));
    } else {
        panic!("both profile surfaces must issue scoped requests");
    }
    a.apply(Event::Profile {
        user: "200".into(),
        result: Ok(view.profile.clone()),
    });
    assert!(a.app.state.panel_profile.as_ref().unwrap().loading);
    assert!(a.app.state.profile.as_ref().unwrap().loading);
    a.apply(repair1_profile_response(
        panel,
        false,
        Ok(view.profile.clone()),
    ));
    assert!(!a.app.state.panel_profile.as_ref().unwrap().loading);
    assert!(
        a.app.state.profile.as_ref().unwrap().loading,
        "the panel response must not complete the simultaneous card"
    );
    a.apply(repair1_profile_response(
        card,
        false,
        Ok(view.profile.clone()),
    ));
    assert!(!a.app.state.profile.as_ref().unwrap().loading);
    assert!(!a.app.state.profile.as_ref().unwrap().large);
    let mut pictures = HashMap::new();
    assert!(a.app.state.profile.is_some());

    let data = profile::profile_data(&mut pictures, &a.app.state, &view);
    assert_eq!(data.name_font, 1);
    assert_eq!(data.name_effect, 8);
    assert_eq!(data.name_colors.row_count(), 2);
    let mut plain = view.clone();
    plain.profile.name_style = None;
    let reset = profile::profile_data(&mut pictures, &a.app.state, &plain);
    assert_eq!(reset.name_font, 0);
    assert_eq!(reset.name_effect, 0);
    assert_eq!(reset.name_colors.row_count(), 0);
    window.set_workspace(true);
    window.set_trusted(true);
    a.refresh(&window);
    assert!(a.app.state.profile.is_some());
    assert_eq!(window.get_profile().name_font, 1);
    assert_eq!(window.get_panel_profile().name_font, 1);
    drop(a);
    window.show().unwrap();
    i_slint_core::window::WindowInner::from_pub(window.window()).ensure_tree_instantiated();
    let inner = i_slint_core::window::WindowInner::from_pub(window.window());
    let found =
        i_slint_core::item_tree::ItemRc::new_root(inner.component()).visit_descendants(|item| {
            if native_name_glyph(&item, "Ada").is_some_and(|glyph| glyph.family == "Bangers") {
                return std::ops::ControlFlow::Break(());
            }
            std::ops::ControlFlow::Continue(())
        });
    assert!(
        found.is_some(),
        "actual native profile name uses the received OFL font"
    );
    // Inspect the real glyph's generated Transform parent, not exposed model
    // scale properties (Slint can otherwise silently ignore root transforms).
    let scales = || {
        let mut values = Vec::new();
        let _: Option<()> = i_slint_core::item_tree::ItemRc::new_root(inner.component())
            .visit_descendants(|item| {
                if native_name_glyph(&item, "Ada").is_some() {
                    let mut parent = item.parent_item(
                        i_slint_core::item_tree::ParentItemTraversalMode::StopAtPopups,
                    );
                    for _ in 0..4 {
                        let Some(p) = parent else {
                            break;
                        };
                        if let Some(t) = p.downcast::<i_slint_core::items::Transform>() {
                            values.push((
                                i_slint_core::items::Transform::FIELD_OFFSETS
                                    .transform_scale_x()
                                    .apply_pin(t.as_pin_ref())
                                    .get(),
                                i_slint_core::items::Transform::FIELD_OFFSETS
                                    .transform_scale_y()
                                    .apply_pin(t.as_pin_ref())
                                    .get(),
                            ));
                            break;
                        }
                        parent = p.parent_item(
                            i_slint_core::item_tree::ParentItemTraversalMode::StopAtPopups,
                        );
                    }
                }
                std::ops::ControlFlow::Continue(())
            });
        values
    };
    ANIMATE.set(Animate::Always);
    window.global::<Theme>().set_reduced_motion(false);
    show_motion(&window);
    let before = scales();
    assert!(
        !before.is_empty(),
        "real profile glyph has generated transform"
    );
    slint::platform::update_timers_and_animations();
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(80));
    slint::platform::update_timers_and_animations();
    let moving = scales();
    assert!(
        moving.iter().any(|(x, y)| *x > 1. && *y < 1.),
        "Gummy changes real child transform: {moving:?}"
    );
    window.global::<Theme>().set_reduced_motion(true);
    show_motion(&window);
    assert!(
        scales().iter().all(|(x, y)| *x == 1. && *y == 1.),
        "reduced motion restores real child geometry"
    );
    // Exercise the actual enlargement callback and inspect its distinct 26px
    // glyph, so the panel's retained 20px name cannot satisfy this assertion.
    window.invoke_enlarge_profile();
    {
        let mut a = adapter.borrow_mut();
        assert!(a.app.state.profile.as_ref().unwrap().large);
        assert!(a.app.state.panel_profile.is_some());
        a.refresh(&window);
    }
    inner.ensure_tree_instantiated();
    let full_family = || {
        i_slint_core::item_tree::ItemRc::new_root(inner.component()).visit_descendants(|item| {
            if let Some(glyph) = native_name_glyph(&item, "Ada") {
                if (glyph.size - 26.).abs() < 0.1 {
                    println!("actual enlarged name glyph: {glyph:?}");
                    return std::ops::ControlFlow::Break(glyph.family);
                }
            }
            std::ops::ControlFlow::Continue(())
        })
    };
    assert_eq!(
        full_family().unwrap(),
        "Bangers",
        "actual enlarged profile font"
    );
    {
        let mut a = adapter.borrow_mut();
        a.app.state.profile.as_mut().unwrap().profile.name_style = None;
        a.refresh(&window);
    }
    let restored = full_family().expect("full profile remains materialized after absent style");
    assert_ne!(
        restored, "Bangers",
        "absent received style restores actual ordinary glyph family"
    );
    assert!(!restored.is_empty());
}
