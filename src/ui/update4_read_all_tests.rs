//! Actual account popover -> production Application -> anonymous command sink.
use super::*;
use i_slint_core::{
    accessibility::{AccessibilityAction, AccessibleStringProperty},
    item_tree::ItemRc,
};
fn find(window: &YapfastWindow, label: &str) -> Option<ItemRc> {
    let inner = i_slint_core::window::WindowInner::from_pub(window.window());
    inner.ensure_tree_instantiated();
    let roots = std::iter::once(inner.component())
        .chain(inner.active_popups().iter().map(|p| p.component.clone()))
        .collect::<Vec<_>>();
    roots.into_iter().find_map(|tree| {
        ItemRc::new_root(tree).visit_descendants(|item| {
            if item.is_visible()
                && item
                    .accessible_string_property(AccessibleStringProperty::Label)
                    .as_deref()
                    .is_some_and(|s| s.contains(label))
            {
                std::ops::ControlFlow::Break(item.clone())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        })
    })
}
#[test]
fn account_menu_read_all_review_cancel_and_confirm_use_anonymous_sink() {
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
    let (backend, mut commands) = Backend::anonymous_fixture(adapter.borrow().runtime.clone());
    {
        let mut a = adapter.borrow_mut();
        let mut snapshot = yapfast_discord::service::Snapshot {
            status: yapfast_discord::service::Status::Online,
            user_id: Some("100".into()),
            ..Default::default()
        };
        snapshot
            .conversations
            .push(Arc::new(yapfast_discord::service::Conversation {
                id: "10".into(),
                name: "received unread".into(),
                ..Default::default()
            }));
        a.apply(Event::Discord(Arc::new(snapshot)));
        a.app.state.account = Some("100".into());
        a.app.state.trusted = true;
        for room in &mut a.app.state.rooms {
            Arc::make_mut(room).unread = 1;
        }
        a.backend = Some(backend);
        a.refresh(&window);
    }
    window.show().unwrap();
    window
        .window()
        .set_size(slint::LogicalSize::new(1120., 760.));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    find(&window, "Set your status")
        .expect("account control")
        .accessible_action(&AccessibilityAction::Default);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    let ask =
        find(&window, "Read all notifications").expect("reachable actual account-menu read review");
    while commands.try_recv().is_ok() {}
    ask.accessible_action(&AccessibilityAction::Default);
    drop(ask);
    assert!(
        commands.try_recv().is_err(),
        "review has no mark-read effect"
    );
    find(&window, "Cancel")
        .expect("cancel")
        .accessible_action(&AccessibilityAction::Default);
    assert!(commands.try_recv().is_err());
    find(&window, "Read all notifications")
        .unwrap()
        .accessible_action(&AccessibilityAction::Default);
    find(&window, "Mark read")
        .expect("visible confirmation")
        .accessible_action(&AccessibilityAction::Default);
    let sent: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok())
        .filter(|c| matches!(c, Command::MarkRead { .. }))
        .collect();
    assert_eq!(sent.len(), 1);
}
