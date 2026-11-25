//! Safe in-memory actual-main appearance acceptance; no live provider or IO.
use super::update4_appearance_tests::{
    native_name_glyph, repair1_profile_command, repair1_profile_response,
};
use super::*;
use i_slint_core::{accessibility::AccessibleStringProperty, item_tree::ItemRc};
use std::ops::ControlFlow;

fn settle(window: &YapfastWindow) {
    i_slint_core::window::WindowInner::from_pub(window.window()).ensure_tree_instantiated();
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    slint::platform::update_timers_and_animations();
    i_slint_core::properties::ChangeTracker::run_change_handlers();
}
fn install(owner: &Rc<RefCell<Adapter>>, window: &YapfastWindow, name: &str) {
    install_profile(
        owner,
        window,
        yapfast_core::chat::Profile {
            id: "200".into(),
            name: name.into(),
            ..Default::default()
        },
        false,
    );
}
fn install_profile(
    owner: &Rc<RefCell<Adapter>>,
    window: &YapfastWindow,
    profile: yapfast_core::chat::Profile,
    panel: bool,
) {
    // Refresh derives workspace/trust from Application, not window overrides.
    // Publish metadata through this exact surface's real scoped request. Moving
    // a card into panel_profile leaves no panel request: show_panel_profile()
    // correctly replaces that forged panel with a new loading shell on dispatch.
    received(owner);
    let mut a = owner.borrow_mut();
    a.layout.sound_volume = Some(0);
    a.app.dispatch(Action::ShowProfilePanel(false));
    let command = repair1_profile_command(a.app.dispatch(if panel {
        Action::ShowProfilePanel(true)
    } else {
        Action::ShowProfile {
            user: profile.id.clone(),
            large: false,
        }
    }));
    let Command::Discord(yapfast_core::chat::DiscordCommand::ScopedProfile(request)) = &command
    else {
        panic!("real scoped profile request");
    };
    assert_eq!(request.panel, panel);
    assert_eq!(request.room, "discord:@me:10");
    assert_eq!(request.owner.as_deref(), Some("100"));
    a.apply(repair1_profile_response(
        command,
        false,
        Ok(profile.clone()),
    ));
    let view = if panel {
        a.app.state.panel_profile.as_ref()
    } else {
        a.app.state.profile.as_ref()
    }
    .unwrap();
    assert!(!view.loading);
    assert!(!view.kept);
    assert_eq!(view.profile.id, profile.id);
    assert_eq!(view.profile.name, profile.name);
    assert_eq!(view.profile.avatar, profile.avatar);
    assert_eq!(view.profile.banner, profile.banner);
    assert_eq!(view.profile.decoration, profile.decoration);
    a.refresh(window);
    assert!(window.get_workspace());
    assert!(window.get_trusted());
    assert_eq!(window.get_profile_open(), !panel);
    if panel {
        assert!(a.app.state.profile_panel);
        assert_eq!(window.get_panel_profile().id, profile.id);
    }
}
fn texts(window: &YapfastWindow, name: &str) -> Vec<(String, slint::Brush)> {
    let inner = i_slint_core::window::WindowInner::from_pub(window.window());
    let mut out = Vec::new();
    let _: Option<()> = ItemRc::new_root(inner.component()).visit_descendants(|item| {
        if let Some(glyph) = native_name_glyph(&item, name) {
            out.push((glyph.family, glyph.brush));
        }
        ControlFlow::Continue(())
    });
    out
}
fn assert_plain_name(window: &YapfastWindow, name: &str, size: f32) {
    let inner = i_slint_core::window::WindowInner::from_pub(window.window());
    let mut glyphs = Vec::new();
    let _: Option<()> = ItemRc::new_root(inner.component()).visit_descendants(|item| {
        if let Some(glyph) = native_name_glyph(&item, name) {
            glyphs.push(glyph);
        }
        ControlFlow::Continue(())
    });
    assert_eq!(
        glyphs.len(),
        1,
        "one actual ordinary name glyph: {glyphs:?}"
    );
    assert_eq!(glyphs[0].kind, "TextInput", "plain name remains selectable");
    assert!(
        (glyphs[0].size - size).abs() < 0.1,
        "ordinary size: {glyphs:?}"
    );
    assert!(
        !glyphs[0].family.is_empty(),
        "resolved ordinary font: {glyphs:?}"
    );
    assert_eq!(
        accessible_count(window, name),
        1,
        "one accessible ordinary name value"
    );
    println!("actual ordinary name glyph: {glyphs:?}");
}
pub(super) fn cpu_snapshot(w: &YapfastWindow) -> slint::SharedPixelBuffer<slint::Rgba8Pixel> {
    use i_slint_core::renderer::RendererSealed;
    for _ in 0..2 {
        i_slint_core::properties::ChangeTracker::run_change_handlers();
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(150));
        slint::platform::update_timers_and_animations();
        i_slint_core::window::WindowInner::from_pub(w.window()).ensure_tree_instantiated();
    }
    let renderer = i_slint_renderer_skia::SkiaRenderer::default_software(&Default::default());
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    renderer.set_window_adapter(&inner.window_adapter());
    let raster = renderer
        .take_snapshot()
        .expect("in-memory CPU Skia raster, no native surface/capture");
    println!("CPU raster {}x{}", raster.width(), raster.height());
    raster
}
fn accessible_count(window: &YapfastWindow, name: &str) -> usize {
    let inner = i_slint_core::window::WindowInner::from_pub(window.window());
    let mut count = 0;
    let _: Option<()> = ItemRc::new_root(inner.component()).visit_descendants(|item| {
        // Plain names are read-only inputs: "Name" is their label and the
        // display name is their accessible Value. Styled Text uses Label.
        // Count actual accessible nodes once, including both rendering paths.
        let label = item.accessible_string_property(AccessibleStringProperty::Label);
        let plain_name = item.downcast::<i_slint_core::items::TextInput>().is_some()
            && label.as_deref() == Some("Name")
            && item
                .accessible_string_property(AccessibleStringProperty::ReadOnly)
                .as_deref()
                == Some("true")
            && item
                .accessible_string_property(AccessibleStringProperty::Value)
                .as_deref()
                == Some(name);
        if item.is_accessible() && (label.as_deref() == Some(name) || plain_name) {
            count += 1;
        }
        ControlFlow::Continue(())
    });
    count
}
#[test]
fn all_received_families_brushes_unicode_and_single_accessible_name() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    bind_callbacks(&window, &owner);
    let name = "Élodie 日本語 العربية 🦊 — a deliberately very long received display name";
    install(&owner, &window, name);
    window.show().unwrap();
    settle(&window);
    assert_plain_name(&window, name, 20.);
    let app_family = texts(&window, name)
        .first()
        .expect("received ordinary profile name is materialized before applying styles")
        .0
        .clone();
    for (id, family) in [
        (1, "Bangers"),
        (2, "BioRhyme"),
        (3, "Cherry Bomb One"),
        (4, "Chicle"),
        (6, "MuseoModerno"),
        (8, "Pixelify Sans"),
        (12, "Zilla Slab"),
        (13, "Playpen Sans"),
        (14, "Orbitron"),
        (15, "New Rocker"),
        (16, "Kalam"),
        (0, ""),
        (11, ""),
        (999, ""),
    ] {
        for (effect, color_count) in (0..=9).flat_map(|e| (0..=5).map(move |c| (e, c))) {
            {
                let mut a = owner.borrow_mut();
                a.app.state.profile.as_mut().unwrap().profile.name_style =
                    Some(yapfast_core::appearance::NameStyle {
                        font_id: id,
                        effect_id: effect,
                        colors: vec![0xe72b51, 0x197be3, 0x54bc38, 0xf9d64a, 0x983ed1]
                            .into_iter()
                            .take(color_count)
                            .collect(),
                    });
                a.refresh(&window);
            }
            settle(&window);
            if effect == 0 && color_count == 5 {
                let raster = cpu_snapshot(&window);
                let pink = raster
                    .as_slice()
                    .iter()
                    .filter(|p| p.r == 0xe7 && p.g == 0x2b && p.b == 0x51 && p.a == 255)
                    .count();
                assert!(
                    pink > 2,
                    "actual CPU raster glyph ink for family {id}: {pink}; native glyphs {:?}",
                    texts(&window, name)
                );
            }
            let glyphs = texts(&window, name);
            assert!(!glyphs.is_empty(), "actual profile glyph {id}/{effect}");
            assert_eq!(
                glyphs.len(),
                if effect == 3 || effect == 5 { 2 } else { 1 },
                "only halo/shadow duplicate glyphs {id}/{effect}"
            );
            assert!(
                glyphs.iter().all(|(f, _)| !f.is_empty()
                    && if family.is_empty() {
                        f == &app_family
                    } else {
                        f == family
                    }),
                "actual child family {id}: {glyphs:?}"
            );
            let brush = &glyphs.last().unwrap().1;
            assert_eq!(
                matches!(brush, slint::Brush::LinearGradient(_)),
                effect == 4 || color_count > 1 && matches!(effect, 2 | 6 | 7 | 8),
                "actual foreground brush {id}/{effect}: {brush:?}"
            );
            assert_eq!(
                accessible_count(&window, name),
                1,
                "exact single textual accessible name {id}/{effect}"
            );
        }
    }
    {
        let mut a = owner.borrow_mut();
        a.app.state.profile.as_mut().unwrap().profile.name_style = None;
        a.refresh(&window);
    }
    settle(&window);
    assert_plain_name(&window, name, 20.);
    assert_eq!(texts(&window, name).len(), 1);
    assert_eq!(accessible_count(&window, name), 1);
    window.invoke_close_profile();
    settle(&window);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(250));
    settle(&window);
    assert!(texts(&window, name).is_empty());
    assert_eq!(accessible_count(&window, name), 0);
}

fn received(owner: &Rc<RefCell<Adapter>>) {
    let mut s = yapfast_discord::service::Snapshot {
        status: yapfast_discord::service::Status::Online,
        user_id: Some("100".into()),
        ..Default::default()
    };
    s.conversations
        .push(Arc::new(yapfast_discord::service::Conversation {
            id: "10".into(),
            name: "received".into(),
            person: Some("200".into()),
            ..Default::default()
        }));
    let mut a = owner.borrow_mut();
    a.apply(Event::Discord(Arc::new(s)));
    a.app.state.account = Some("100".into());
    a.app.state.selected = Some("discord:@me:10".into());
    a.app.state.navigation = yapfast_core::application::Navigation::DirectMessages;
    a.app.state.trusted = true;
    a.app.state.dm_screen = None;
}
fn decoration_pixels(window: &YapfastWindow, expected: [u8; 4], side: f32) -> usize {
    let inner = i_slint_core::window::WindowInner::from_pub(window.window());
    let mut count = 0;
    let _: Option<()> = ItemRc::new_root(inner.component()).visit_descendants(|item| {
        if let Some(i) = item.downcast::<i_slint_core::items::ImageItem>() {
            let src = i_slint_core::items::ImageItem::FIELD_OFFSETS
                .source()
                .apply_pin(i.as_pin_ref())
                .get();
            if src.size().width == 3 {
                let b = src.to_rgba8().unwrap();
                assert_eq!(&b.as_bytes()[..4], &expected);
                assert_eq!(b.as_bytes()[11], 0);
                let g = item.geometry();
                assert!(
                    (g.width() - side).abs() < 0.1,
                    "overlay width {g:?} expected {side}"
                );
                assert!((g.height() - side).abs() < 0.1);
                count += 1;
            }
        }
        ControlFlow::Continue(())
    });
    count
}
fn picture(red: u8) -> Arc<Picture> {
    let mut png = std::io::Cursor::new(Vec::new());
    let mut rgba = image::RgbaImage::from_pixel(3, 3, image::Rgba([red, 231, 99, 255]));
    rgba.put_pixel(2, 0, image::Rgba([0, 0, 0, 0]));
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    Arc::new(yapfast_core::chat::decode_picture(png.get_ref(), 128, 128).unwrap())
}
#[test]
fn full_and_dm_panel_decoration_replacement_expiry_failure_and_release() {
    i_slint_backend_testing::init_no_event_loop();
    for panel in [false, true] {
        let d = tempfile::tempdir().unwrap();
        let w = YapfastWindow::new().unwrap();
        let a = Adapter::new(
            Arc::new(BackgroundRuntime::new().unwrap()),
            d.path().into(),
            &w,
        )
        .unwrap();
        bind_callbacks(&w, &a);
        let parsed = yapfast_discord::profile::parse(
            &serde_json::json!({
                "user": {
                    "id": "200", "username": "overlay acceptance",
                    "avatar": "0123456789abcdef0123456789abcdef",
                    "banner": "0123456789abcdef0123456789abcdef",
                    "avatar_decoration_data": {"sku_id": "123", "asset": "safe"}
                }
            }),
            None,
        )
        .unwrap();
        let art: yapfast_core::appearance::Decoration = parsed.decoration.unwrap().into();
        let url = art.url.clone();
        let received_profile = yapfast_core::chat::Profile {
            id: "200".into(),
            name: "overlay acceptance".into(),
            avatar: Some(parsed.avatar),
            banner: parsed.banner,
            decoration: Some(art),
            ..Default::default()
        };
        // Both originals must be accepted identities before any native action;
        // decoration art is not an original-media resource.
        for kind in [ProfilePicture::Avatar, ProfilePicture::Banner] {
            let asset =
                yapfast_core::profile_media::Asset::from_profile("100", &received_profile, kind)
                    .expect("parsed received avatar/banner is a valid original-media identity");
            assert_eq!(asset.account(), "100");
            assert_eq!(asset.user(), "200");
            assert_eq!(asset.kind(), kind);
            assert!(asset.url().ends_with("?size=4096"));
            assert!(!asset.url().contains("avatar-decoration"));
        }
        install_profile(&a, &w, received_profile.clone(), panel);
        let first = picture(17);
        let weak = Arc::downgrade(&first);
        {
            let mut o = a.borrow_mut();
            o.app.state.pictures.insert(url.clone(), first);
            let v = if panel {
                o.app.state.panel_profile.as_mut()
            } else {
                o.app.state.profile.as_mut()
            }
            .unwrap();
            v.large = !panel;
            o.refresh(&w);
        }
        w.show().unwrap();
        settle(&w);
        let side = if panel { 96. } else { 115.2 };
        assert_eq!(
            decoration_pixels(&w, [17, 231, 99, 255], side),
            1,
            "actual requested surface overlay"
        );
        let raster = cpu_snapshot(&w);
        assert!(
            raster
                .as_slice()
                .iter()
                .filter(|p| p.r == 17 && p.g == 231 && p.b == 99)
                .count()
                > 10,
            "actual full/panel CPU raster contains distinctive transparent art"
        );
        drop(raster);
        for kind in ["avatar", "banner"] {
            let media_kind = if kind == "avatar" {
                ProfilePicture::Avatar
            } else {
                ProfilePicture::Banner
            };
            let expected_asset = yapfast_core::profile_media::Asset::from_profile(
                "100",
                &received_profile,
                media_kind,
            )
            .unwrap();
            {
                let o = a.borrow();
                let v = if panel {
                    o.app.state.panel_profile.as_ref()
                } else {
                    o.app.state.profile.as_ref()
                }
                .unwrap();
                assert!(
                    !v.loading,
                    "accepted surface remains loaded before {panel}/{kind}"
                );
                assert_eq!(
                    yapfast_core::profile_media::Asset::from_profile("100", &v.profile, media_kind),
                    Some(expected_asset.clone())
                );
                let projected = if panel {
                    w.get_panel_profile()
                } else {
                    w.get_profile()
                };
                assert!(projected.avatar_media && projected.banner_media);
            }
            let inner = i_slint_core::window::WindowInner::from_pub(w.window());
            let label = format!("View overlay acceptance's {kind}");
            let target = ItemRc::new_root(inner.component())
                .visit_descendants(|i| {
                    if i.accessible_string_property(AccessibleStringProperty::Label)
                        .as_deref()
                        == Some(label.as_str())
                    {
                        ControlFlow::Break(i.clone())
                    } else {
                        ControlFlow::Continue(())
                    }
                })
                .expect("original media control remains present under decoration");
            assert!(target.is_accessible());
            target.accessible_action(&i_slint_core::accessibility::AccessibilityAction::Default);
            drop(target);
            settle(&w);
            assert!(
                w.get_viewer_open(),
                "native original-media action {panel}/{kind}"
            );
            assert!(w.get_viewer_profile_media());
            {
                let o = a.borrow();
                let viewer = o
                    .app
                    .state
                    .viewer
                    .as_ref()
                    .expect("native action creates a viewer session");
                let yapfast_core::application::ViewerSource::Profile(session) = &viewer.source
                else {
                    panic!("original media must not create a message viewer");
                };
                assert_eq!(
                    session.slot,
                    if panel {
                        yapfast_core::profile_media::Slot::Panel
                    } else {
                        yapfast_core::profile_media::Slot::Profile
                    }
                );
                assert_eq!(session.asset, expected_asset);
                let v = if panel {
                    o.app.state.panel_profile.as_ref()
                } else {
                    o.app.state.profile.as_ref()
                }
                .unwrap();
                assert!(
                    !v.loading,
                    "native action must not replace the accepted scoped profile"
                );
                assert_eq!(v.profile.name, "overlay acceptance");
            }
            assert_eq!(w.get_viewer_media_link(), expected_asset.url());
            assert!(w.get_viewer_media_link().contains(if kind == "avatar" {
                "/avatars/"
            } else {
                "/banners/"
            }));
            assert!(!w.get_viewer_media_link().contains("avatar-decoration"));
            w.invoke_close_image();
            settle(&w);
            assert!(!w.get_viewer_open());
        }
        let mut retained = if panel {
            w.get_panel_profile()
        } else {
            w.get_profile()
        }
        .decoration
        .to_rgba8()
        .unwrap();
        let second = picture(211);
        let second_weak = Arc::downgrade(&second);
        {
            let mut o = a.borrow_mut();
            let replacement_url = url.replace("/safe.png", "/next.png");
            o.app.apply(Event::Picture {
                url: replacement_url.clone(),
                picture: second,
            });
            let v = if panel {
                o.app.state.panel_profile.as_mut()
            } else {
                o.app.state.profile.as_mut()
            }
            .unwrap();
            v.profile.decoration.as_mut().unwrap().url = replacement_url;
            o.refresh(&w);
        }
        settle(&w);
        assert_eq!(
            decoration_pixels(&w, [211, 231, 99, 255], side),
            1,
            "same-size source replacement"
        );

        let mut second_pixels = if panel {
            w.get_panel_profile()
        } else {
            w.get_profile()
        }
        .decoration
        .to_rgba8()
        .unwrap();
        for expired in [true, false] {
            let mut o = a.borrow_mut();
            let v = if panel {
                o.app.state.panel_profile.as_mut()
            } else {
                o.app.state.profile.as_mut()
            }
            .unwrap();
            v.profile.decoration.as_mut().unwrap().expires_at = expired.then_some(1);
            o.refresh(&w);
            drop(o);
            settle(&w);
            assert_eq!(
                if panel {
                    w.get_panel_profile()
                } else {
                    w.get_profile()
                }
                .decoration
                .size()
                .width,
                if expired { 0 } else { 3 }
            );
        }
        {
            let mut o = a.borrow_mut();
            let v = if panel {
                o.app.state.panel_profile.as_mut()
            } else {
                o.app.state.profile.as_mut()
            }
            .unwrap();
            v.profile.decoration.as_mut().unwrap().url = "missing-generated-asset".into();
            o.refresh(&w);
        }
        settle(&w);
        assert_eq!(
            if panel {
                w.get_panel_profile()
            } else {
                w.get_profile()
            }
            .decoration
            .size()
            .width,
            0,
            "missing/failure art never reuses old asset"
        );
        {
            let mut o = a.borrow_mut();
            if panel {
                o.app.dispatch(Action::ShowProfilePanel(false));
            } else {
                o.app.dispatch(Action::CloseProfile);
            }
            for i in 0..401 {
                o.app.apply(Event::Picture {
                    url: format!("ordinary-pressure-{i}"),
                    picture: Arc::new(Picture::default()),
                });
            }
            o.refresh(&w);
            assert!(!o.app.state.pictures.contains_key(&url));
        }
        settle(&w);
        assert_eq!(w.get_profile().decoration.size().width, 0);
        assert_eq!(w.get_panel_profile().decoration.size().width, 0);
        assert!(weak.upgrade().is_none());
        assert!(second_weak.upgrade().is_none());
        let second_pointer = second_pixels.as_bytes().as_ptr();
        assert_eq!(
            second_pixels.make_mut_bytes().as_ptr(),
            second_pointer,
            "replacement/expired/failed overlay releases without COW"
        );
        let p = retained.as_bytes().as_ptr();
        assert_eq!(
            retained.make_mut_bytes().as_ptr(),
            p,
            "retired old overlay releases pixels under ordinary pressure without COW"
        );
    }
}

fn scales(w: &YapfastWindow, name: &str) -> Vec<(f32, f32)> {
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    let mut v = Vec::new();
    let _: Option<()> = ItemRc::new_root(inner.component()).visit_descendants(|i| {
        if native_name_glyph(&i, name).is_some() {
            let mut p =
                i.parent_item(i_slint_core::item_tree::ParentItemTraversalMode::StopAtPopups);
            for _ in 0..4 {
                let Some(item) = p else {
                    break;
                };
                if let Some(t) = item.downcast::<i_slint_core::items::Transform>() {
                    v.push((
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
                p = item
                    .parent_item(i_slint_core::item_tree::ParentItemTraversalMode::StopAtPopups);
            }
        }
        ControlFlow::Continue(())
    });
    v
}
#[test]
fn actual_backend_name_motion_reset_close_and_drop_quiesce() {
    i_slint_backend_testing::init_no_event_loop();
    let d = tempfile::tempdir().unwrap();
    let w = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        d.path().into(),
        &w,
    )
    .unwrap();
    bind_callbacks(&w, &owner);
    install(&owner, &w, "motion Unicode 日本語");
    ANIMATE.set(Animate::Always);
    w.show().unwrap();
    show_motion(&w);
    settle(&w);
    // The ordinary selectable name intentionally has no Transform. Establish
    // its real glyph/accessibility presence; require a Transform only once the
    // styled Gummy branch has actually been projected below.
    assert_plain_name(&w, "motion Unicode 日本語", 20.);
    // The first refresh starts an unrelated one-shot roster coalescing timer,
    // even with no members. Let it finish naturally before measuring name work;
    // do not stop timers or accept a nonempty baseline.
    assert!(owner.borrow().roster_due.running());
    i_slint_backend_testing::mock_elapsed_time(refresh::ROSTER_PAUSE);
    settle(&w);
    assert!(!owner.borrow().roster_due.running());
    let baseline = slint::platform::duration_until_next_timer_update();
    assert_eq!(
        baseline, None,
        "empty native timer census before name effect"
    );
    {
        let mut a = owner.borrow_mut();
        a.app.state.profile.as_mut().unwrap().profile.name_style =
            Some(yapfast_core::appearance::NameStyle {
                font_id: 1,
                effect_id: 8,
                colors: vec![0xe72b51, 0x197be3],
            });
        a.refresh(&w);
    }
    settle(&w);
    assert!(
        !scales(&w, "motion Unicode 日本語").is_empty(),
        "actual styled profile glyph transform exists before motion checks"
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(80));
    slint::platform::update_timers_and_animations();
    assert!(
        scales(&w, "motion Unicode 日本語")
            .iter()
            .any(|(x, y)| *x > 1. && *y < 1.)
    );
    i_slint_core::with_existing_context(|c| {
        c.set_motion_preference(i_slint_core::MotionPreference::Reduced)
    })
    .unwrap();
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    settle(&w);
    assert!(
        scales(&w, "motion Unicode 日本語")
            .iter()
            .all(|s| *s == (1., 1.))
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(2));
    slint::platform::update_timers_and_animations();
    assert!(
        scales(&w, "motion Unicode 日本語")
            .iter()
            .all(|s| *s == (1., 1.))
    );
    i_slint_core::with_existing_context(|c| {
        c.set_motion_preference(i_slint_core::MotionPreference::NoPreference)
    })
    .unwrap();
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    settle(&w);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(80));
    slint::platform::update_timers_and_animations();
    assert!(
        scales(&w, "motion Unicode 日本語")
            .iter()
            .any(|s| *s != (1., 1.)),
        "backend preference alone resumes glyph transform"
    );
    {
        let mut a = owner.borrow_mut();
        a.app.state.profile.as_mut().unwrap().profile.name_style = None;
        a.refresh(&w);
    }
    settle(&w);
    assert_plain_name(&w, "motion Unicode 日本語", 20.);
    assert!(
        scales(&w, "motion Unicode 日本語")
            .iter()
            .all(|s| *s == (1., 1.))
    );
    w.invoke_close_profile();
    settle(&w);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(250));
    settle(&w);
    assert!(texts(&w, "motion Unicode 日本語").is_empty());
    let after = slint::platform::duration_until_next_timer_update();
    println!("name timer deadlines baseline {baseline:?}, reset/close {after:?}");
    assert_eq!(after, None, "no remaining timer after reset/close");
    owner.borrow_mut().detach();
    drop(w);
    drop(owner);
    slint::platform::update_timers_and_animations();
    let after_drop = slint::platform::duration_until_next_timer_update();
    println!("drop timer deadline {after_drop:?}");
    assert_eq!(
        after_drop, None,
        "no remaining timer after actual Adapter/window drop"
    );
}

#[test]
fn native_profile_producers_scope_kept_success_error_and_retire_appearance() {
    i_slint_backend_testing::init_no_event_loop();
    for outcome in 0..3 {
        for same_context in [false, true] {
            let d = tempfile::tempdir().unwrap();
            let w = YapfastWindow::new().unwrap();
            let owner = Adapter::new(
                Arc::new(BackgroundRuntime::new().unwrap()),
                d.path().into(),
                &w,
            )
            .unwrap();
            owner.borrow_mut().layout.sound_volume = Some(0);
            bind_callbacks(&w, &owner);
            received(&owner);
            let (backend, mut sink) = Backend::anonymous_fixture(owner.borrow().runtime.clone());
            owner.borrow_mut().backend = Some(backend);
            {
                let mut a = owner.borrow_mut();
                let mut rooms = a.app.state.rooms.to_vec();
                let mut guild = rooms[0].clone();
                Arc::make_mut(&mut guild).id = "discord:20:11".into();
                rooms.push(guild);
                a.app.state.rooms = rooms.into();
                a.refresh(&w);
            }
            owner
                .borrow_mut()
                .action(&w, Action::ShowProfilePanel(false));
            while sink.try_recv().is_ok() {}
            owner
                .borrow_mut()
                .action(&w, Action::ShowProfilePanel(true));
            let panel_request = std::iter::from_fn(|| sink.try_recv().ok())
                .find_map(|c| {
                    if let Command::Discord(yapfast_core::chat::DiscordCommand::ScopedProfile(r)) =
                        c
                    {
                        Some(r)
                    } else {
                        None
                    }
                })
                .expect("real panel producer uses scoped path");
            assert_eq!(panel_request.room, "discord:@me:10");
            owner
                .borrow_mut()
                .action(&w, Action::ShowProfilePanel(false));
            owner
                .borrow_mut()
                .action(&w, Action::ShowDmProfile("discord:@me:10".into()));
            let dm_request = std::iter::from_fn(|| sink.try_recv().ok())
                .find_map(|c| {
                    if let Command::Discord(yapfast_core::chat::DiscordCommand::ScopedProfile(r)) =
                        c
                    {
                        Some(r)
                    } else {
                        None
                    }
                })
                .expect("real DM-card producer uses scoped path");
            assert_eq!(dm_request.room, "discord:@me:10");
            assert!(!Arc::ptr_eq(&dm_request, &panel_request));
            w.invoke_close_profile();
            w.invoke_show_profile("200".into());
            let old = std::iter::from_fn(|| sink.try_recv().ok())
                .find_map(|c| {
                    if let Command::Discord(yapfast_core::chat::DiscordCommand::ScopedProfile(r)) =
                        c
                    {
                        Some(r)
                    } else {
                        None
                    }
                })
                .expect("real native producer dispatches scoped request");
            assert_eq!(old.room, "discord:@me:10");
            w.invoke_close_profile();
            if !same_context {
                owner.borrow_mut().app.state.selected = Some("discord:20:11".into());
            }
            w.invoke_show_profile("200".into());
            let current = std::iter::from_fn(|| sink.try_recv().ok())
                .find_map(|c| {
                    if let Command::Discord(yapfast_core::chat::DiscordCommand::ScopedProfile(r)) =
                        c
                    {
                        Some(r)
                    } else {
                        None
                    }
                })
                .expect("reopened real producer scoped");
            assert!(!Arc::ptr_eq(&old, &current));
            assert_eq!(
                current.room,
                if same_context {
                    "discord:@me:10"
                } else {
                    "discord:20:11"
                }
            );
            let parsed=yapfast_discord::profile::parse(&serde_json::json!({"user":{"id":"200","username":"scoped received","display_name_styles":{"font_id":16,"effect_id":8,"colors":[16711680,255]},"avatar_decoration_data":{"sku_id":"123","asset":"safe"}}}),None).unwrap();
            let styled = yapfast_core::chat::Profile {
                id: "200".into(),
                name: "scoped received".into(),
                name_style: parsed.name_style.map(Into::into),
                decoration: parsed.decoration.map(Into::into),
                ..Default::default()
            };
            let art_url = styled.decoration.as_ref().unwrap().url.clone();
            owner
                .borrow_mut()
                .app
                .state
                .pictures
                .insert(art_url, picture(33));
            for r in [&old, &current] {
                let snapshot = Arc::new(yapfast_discord::service::Snapshot {
                    status: yapfast_discord::service::Status::Online,
                    user_id: r.owner.clone(),
                    presence_observation: r.source.clone(),
                    ..Default::default()
                });
                let (publication, _live) =
                    yapfast_core::chat::ProfilePublication::fixture(snapshot);
                *r.publication.lock().unwrap() = Some(publication);
            }
            {
                let mut a = owner.borrow_mut();
                a.apply(Event::ProfilePresentation {
                    request: old,
                    kept: outcome == 1,
                    result: if outcome == 2 {
                        Err("old context failed".into())
                    } else {
                        Ok(styled.clone())
                    },
                });
                assert!(a.app.state.profile.as_ref().unwrap().loading);
                assert!(
                    a.app
                        .state
                        .profile
                        .as_ref()
                        .unwrap()
                        .profile
                        .name_style
                        .is_none()
                );
                a.refresh(&w);
            }
            settle(&w);
            assert_eq!(w.get_profile().name_font, 0);
            assert_eq!(w.get_profile().decoration.size().width, 0);
            {
                let mut a = owner.borrow_mut();
                a.apply(Event::ProfilePresentation {
                    request: current,
                    kept: false,
                    result: Ok(styled),
                });
                a.refresh(&w);
                assert!(!a.app.state.profile.as_ref().unwrap().loading);
            }
            settle(&w);
            assert_eq!(w.get_profile().decoration.size().width, 3);
            assert_eq!(w.get_profile().name_font, 16);
            assert!(
                texts(&w, "scoped received")
                    .iter()
                    .any(|(f, _)| f == "Kalam")
            );
            // Replacement received account lifetime retires new appearance without
            // destroying inherited cached identity/presence.
            {
                let mut a = owner.borrow_mut();
                a.apply(Event::Discord(Arc::new(
                    yapfast_discord::service::Snapshot {
                        status: yapfast_discord::service::Status::Online,
                        user_id: Some("101".into()),
                        ..Default::default()
                    },
                )));
                a.refresh(&w);
            }
            settle(&w);
            assert_eq!(
                w.get_profile_open(),
                owner.borrow().app.state.profile.is_some()
            );
            if w.get_profile_open() {
                assert_eq!(w.get_profile().name_font, 0);
            }
            assert_eq!(w.get_profile().decoration.size().width, 0);
        }
    }
}
