//! Actual Application/Adapter/window production decoder and EmojiFace consumers.
use super::*;
use i_slint_core::{accessibility::AccessibleStringProperty, item_tree::ItemRc};
use std::ops::ControlFlow;

#[test]
fn actual_timeline_and_picker_share_generated_decoder_frames() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let w = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &w,
    )
    .unwrap();
    owner.borrow_mut().layout.sound_volume = Some(0);
    bind_callbacks(&w, &owner);
    ANIMATE.set(Animate::Always);
    let mut fixture = yapfast_discord::service::ContactSnapshotFixture::new(
        &serde_json::json!({"user":{"id":"1","username":"owned fixture","premium_type":2},"guilds":[{"id":"10","properties":{"name":"owned","owner_id":"1"},"roles":[{"id":"10","permissions":"68608"}],"emojis":[{"id":"220","name":"combined","animated":true,"available":true}],"channels":[{"id":"11","name":"received","type":0},{"id":"12","name":"next received room","type":0}]}],"merged_members":[[{"user_id":"1","roles":[]}]]}),
    );
    let (backend, _sink) = Backend::anonymous_fixture(owner.borrow().runtime.clone());
    owner.borrow_mut().backend = Some(backend);
    let url =
        yapfast_discord::ready::custom_emoji("220", true, yapfast_core::chat::EMOJI_PICTURE_SIZE);
    let key = "combined:220";
    let mut data = Vec::new();
    {
        let mut e = image::codecs::gif::GifEncoder::new(&mut data);
        for (red, delay) in [(37, 40), (199, 90)] {
            e.encode_frame(image::Frame::from_parts(
                image::RgbaImage::from_pixel(4, 4, image::Rgba([red, 31, 71, 255])),
                0,
                0,
                image::Delay::from_numer_denom_ms(delay, 1),
            ))
            .unwrap();
        }
    }
    {
        let mut a = owner.borrow_mut();
        a.apply(Event::Discord(Arc::new(fixture.snapshot())));
        a.app.state.selected = Some("discord:10:11".into());
        a.app.state.room_ready = true;
        a.app.state.trusted = true;
        a.app.state.account = Some("1".into());
        a.app.state.dm_screen = None;
        a.app.state.navigation = yapfast_core::application::Navigation::Server("discord:10".into());
        a.app.state.pictures.insert(
            url.clone(),
            Arc::new(yapfast_core::chat::decode_picture(&data, 128, 128).unwrap()),
        );
        let emoji = yapfast_core::chat::CustomEmoji {
            text: ":combined:".into(),
            id: "220".into(),
            animated: true,
            usable: true,
            picture: url.clone(),
        };
        a.app.state.messages = Arc::new(vec![Arc::new(ChatMessage {
            id: "100".into(),
            item: "d:100".into(),
            author: "fixture".into(),
            body: ":combined:".into(),
            formatted: true,
            emoji: vec![emoji],
            reactions: vec![yapfast_core::chat::Reaction {
                key: key.into(),
                count: 1,
                mine: false,
            }],
            ..Default::default()
        })]);
        a.refresh(&w);
    }
    w.show().unwrap();
    show_motion(&w);
    for _ in 0..200 {
        std::thread::sleep(Duration::from_millis(2));
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));
        slint::platform::update_timers_and_animations();
        if owner.borrow().animations.cache.contains(key) {
            break;
        }
    }
    let weak;
    let mut retained;
    {
        let mut a = owner.borrow_mut();
        let f = a
            .animations
            .cache
            .get(key)
            .expect("production timeline decode");
        assert_eq!(f.delays(), &[40, 90]);
        weak = Rc::downgrade(&f);
        retained = f.images()[0].to_rgba8().unwrap();
    }
    fn settle_app(w: &YapfastWindow) {
        i_slint_core::window::WindowInner::from_pub(w.window()).ensure_tree_instantiated();
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        slint::platform::update_timers_and_animations();
    }
    fn native_images(w: &YapfastWindow) -> Vec<slint::Image> {
        let inner = i_slint_core::window::WindowInner::from_pub(w.window());
        let mut v = Vec::new();
        for tree in std::iter::once(inner.component())
            .chain(inner.active_popups().iter().map(|p| p.component.clone()))
        {
            let _: Option<()> = ItemRc::new_root(tree).visit_descendants(|i| {
                if let Some(img) = i.downcast::<i_slint_core::items::ImageItem>() {
                    let src = i_slint_core::items::ImageItem::FIELD_OFFSETS
                        .source()
                        .apply_pin(img.as_pin_ref())
                        .get();
                    if src.size().width == 48 {
                        v.push(src);
                    }
                }
                ControlFlow::Continue(())
            });
        }
        v
    }
    settle_app(&w);
    let glyph = custom_emoji::glyph("220").expect("actual timeline font registration");
    let by_glyph =
        i_slint_core::yapfast_emoji::frame_of_glyph(glyph).expect("timeline glyph animation");
    assert_eq!(
        by_glyph.to_rgba8().unwrap().as_bytes().as_ptr(),
        i_slint_core::yapfast_emoji::frame_of_key(key)
            .unwrap()
            .to_rgba8()
            .unwrap()
            .as_bytes()
            .as_ptr()
    );
    drop(by_glyph);
    w.invoke_browse_messages();
    for _ in 0..8 {
        let _ = w.invoke_timeline_extent();
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        slint::platform::update_timers_and_animations();
    }
    let raster = super::super::update4_appearance_acceptance_tests::cpu_snapshot(&w);
    assert!(
        raster
            .as_slice()
            .iter()
            .filter(|p| (p.r == 37 || p.r == 199) && p.g == 31 && p.b == 71)
            .count()
            > 10,
        "actual timeline CPU text raster uses generated emoji"
    );
    drop(raster);
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    let open = ItemRc::new_root(inner.component())
        .visit_descendants(|i| {
            if i.accessible_string_property(AccessibleStringProperty::Label)
                .as_deref()
                == Some("Add an emoji")
            {
                ControlFlow::Break(i.clone())
            } else {
                ControlFlow::Continue(())
            }
        })
        .expect("real composer emoji button");
    open.accessible_action(&i_slint_core::accessibility::AccessibilityAction::Default);
    drop(open);
    settle_app(&w);
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    let input = std::iter::once(inner.component())
        .chain(inner.active_popups().iter().map(|p| p.component.clone()))
        .find_map(|tree| {
            ItemRc::new_root(tree).visit_descendants(|i| {
                if i.accessible_string_property(AccessibleStringProperty::Label)
                    .as_deref()
                    == Some("Find an emoji")
                {
                    ControlFlow::Break(i.clone())
                } else {
                    ControlFlow::Continue(())
                }
            })
        })
        .expect("native search input");
    let mut pos = input.map_to_window(Default::default());
    if let Some(p) = inner.active_popups().last() {
        if let i_slint_core::window::PopupWindowLocation::ChildWindow(off) = p.location {
            pos.x += off.x;
            pos.y += off.y;
        }
    }
    let g = input.geometry();
    let position = slint::LogicalPosition::new(pos.x + g.width() / 2., pos.y + g.height() / 2.);
    for event in [
        slint::platform::WindowEvent::PointerPressed {
            position,
            button: slint::platform::PointerEventButton::Left,
        },
        slint::platform::WindowEvent::PointerReleased {
            position,
            button: slint::platform::PointerEventButton::Left,
        },
    ] {
        w.window().dispatch_event(event);
    }
    drop(input);
    for ch in "combined".chars() {
        w.window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: ch.to_string().into(),
            });
        w.window()
            .dispatch_event(slint::platform::WindowEvent::KeyReleased {
                text: ch.to_string().into(),
            });
    }
    settle_app(&w);
    assert_eq!(w.get_emoji_query(), "combined");
    assert!(
        owner.borrow().animations.picker.contains(key),
        "production picker registration"
    );
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    let tile = std::iter::once(inner.component())
        .chain(inner.active_popups().iter().map(|p| p.component.clone()))
        .find_map(|tree| {
            ItemRc::new_root(tree).visit_descendants(|i| {
                if i.is_visible()
                    && i.accessible_string_property(AccessibleStringProperty::Label)
                        .as_deref()
                        == Some("Insert :combined:")
                {
                    ControlFlow::Break(i.clone())
                } else {
                    ControlFlow::Continue(())
                }
            })
        })
        .expect("native picker tile");
    let mut p = tile.map_to_window(Default::default());
    if let Some(popup) = inner.active_popups().last() {
        if let i_slint_core::window::PopupWindowLocation::ChildWindow(offset) = popup.location {
            p.x += offset.x;
            p.y += offset.y;
        }
    }
    let g = tile.geometry();
    w.window()
        .dispatch_event(slint::platform::WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(p.x + g.width() / 2., p.y + g.height() / 2.),
        });
    settle_app(&w);
    assert!(
        std::iter::once(inner.component())
            .chain(inner.active_popups().iter().map(|p| p.component.clone()))
            .any(|tree| ItemRc::new_root(tree)
                .visit_descendants(|i| {
                    if i.is_visible()
                        && i.accessible_string_property(AccessibleStringProperty::Label)
                            .as_deref()
                            == Some(":combined:")
                    {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                })
                .is_some()),
        "hover footer name"
    );
    drop(tile);
    let sources = native_images(&w);
    assert!(!sources.is_empty(), "production picker EmojiFace source");
    let frame = i_slint_core::yapfast_emoji::frame_of_key(key)
        .unwrap()
        .to_rgba8()
        .unwrap();
    for s in &sources {
        assert_eq!(
            s.to_rgba8().unwrap().as_bytes().as_ptr(),
            frame.as_bytes().as_ptr(),
            "actual timeline/picker shared decoder buffer"
        );
    }
    drop(sources);
    drop(frame);
    w.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "\n".into() });
    w.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: "\n".into() });
    settle_app(&w);
    assert!(
        w.get_draft().contains(":combined:"),
        "native keyboard selects local composer emoji"
    );
    let reopen = ItemRc::new_root(inner.component())
        .visit_descendants(|i| {
            if i.accessible_string_property(AccessibleStringProperty::Label)
                .as_deref()
                == Some("Add an emoji")
            {
                ControlFlow::Break(i.clone())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    reopen.accessible_action(&i_slint_core::accessibility::AccessibilityAction::Default);
    drop(reopen);
    w.set_emoji_query("combined".into());
    w.invoke_emoji_search("combined".into());
    settle_app(&w);
    assert!(!native_images(&w).is_empty());
    i_slint_core::with_existing_context(|c| {
        c.set_motion_preference(i_slint_core::MotionPreference::Reduced)
    })
    .unwrap();
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    assert!(!owner.borrow().animations.timer.running());
    i_slint_core::with_existing_context(|c| {
        c.set_motion_preference(i_slint_core::MotionPreference::NoPreference)
    })
    .unwrap();
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    assert!(owner.borrow().animations.timer.running());
    let rev = i_slint_core::yapfast_emoji::revision();
    std::thread::sleep(Duration::from_millis(50));
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(50));
    slint::platform::update_timers_and_animations();
    settle_app(&w);
    assert!(i_slint_core::yapfast_emoji::revision() > rev);
    assert!(
        native_images(&w)
            .iter()
            .any(|s| s.to_rgba8().unwrap().as_bytes()[0] == 199),
        "event-only actual rendered resumed pixels"
    );
    let raster = super::super::update4_appearance_acceptance_tests::cpu_snapshot(&w);
    assert!(
        raster
            .as_slice()
            .iter()
            .filter(|p| p.r == 199 && p.g == 31 && p.b == 71)
            .count()
            > 10,
        "actual CPU scene has resumed generated frame, no manual lifecycle sync"
    );
    drop(raster);
    w.set_react_item("yapfast:composer".into());
    w.invoke_emoji_search("combined".into());
    let n = w.get_emoji_rows().row_count();
    w.invoke_emoji_scrolled_to(100000., 50.);
    assert!(
        owner.borrow().animations.picker.is_empty(),
        "known offscreen picker retires consumer"
    );
    w.invoke_emoji_search("combined".into());
    assert!(owner.borrow().animations.picker.contains(key));
    let mut changed = fixture.snapshot();
    Arc::make_mut(&mut Arc::make_mut(&mut changed.servers[0].server).emojis)[0].id = "221".into();
    {
        let mut a = owner.borrow_mut();
        a.apply(Event::Discord(Arc::new(changed)));
        a.refresh(&w);
    }
    w.invoke_emoji_search("combined".into());
    assert_eq!(w.get_emoji_rows().row_count(), n);
    assert!(owner.borrow().animations.picker.contains("combined:221"));
    assert!(
        !owner.borrow().animations.picker.contains(key),
        "same-count received source rebuild replaces consumer identity"
    );
    w.invoke_emoji_search("no-match-combined-xyz".into());
    settle_app(&w);
    assert!(owner.borrow().animations.picker.is_empty());
    assert_eq!(w.get_emoji_rows().row_count(), 0);
    w.invoke_select_room("discord:10:12".into());
    assert!(owner.borrow().app.state.messages.is_empty());
    {
        let a = owner.borrow();
        assert!(
            a.animations.cache.pinned.is_empty(),
            "real room switch retires timeline consumer"
        );
    }
    let mut next = fixture.snapshot();
    next.user_id = Some("101".into());
    {
        let mut a = owner.borrow_mut();
        a.apply(Event::Discord(Arc::new(next)));
        a.refresh(&w);
        assert!(
            a.animations.cache.pinned.is_empty() && a.animations.picker.is_empty(),
            "account replacement cannot revive old consumers"
        );
        for i in 0..4 {
            a.animations.cache.insert(
                format!("ordinary-pressure:{i}"),
                None,
                Rc::new(Frames::new(
                    vec![slint::SharedPixelBuffer::new(1024, 1024)],
                    &[40],
                )),
            );
            a.animations.cache.evict();
        }
    }
    settle_app(&w);
    assert!(weak.upgrade().is_none());
    let p = retained.as_bytes().as_ptr();
    assert_eq!(
        retained.make_mut_bytes().as_ptr(),
        p,
        "real app drops every shared pixel owner without COW"
    );
    owner.borrow_mut().detach();
    drop(w);
    assert!(!owner.borrow().animations.timer.running());
}
