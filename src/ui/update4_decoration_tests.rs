use super::*;
#[test]
fn decoration_overlay_is_received_static_and_expiry_safe() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    owner.borrow_mut().layout.sound_volume = Some(0);
    bind_callbacks(&window, &owner);
    let parsed = yapfast_discord::profile::parse(&serde_json::json!({"user":{"id":"200","username":"Ada","avatar_decoration_data":{"sku_id":"123","asset":"safe"}}}), None).unwrap();
    let mut a = owner.borrow_mut();
    let decoration: yapfast_core::appearance::Decoration = parsed.decoration.unwrap().into();
    let url = decoration.url.clone();
    let mut png = std::io::Cursor::new(Vec::new());
    let mut rgba = image::RgbaImage::from_pixel(2, 2, image::Rgba([17, 231, 99, 255]));
    rgba.put_pixel(1, 0, image::Rgba([0, 0, 0, 0]));
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let picture = Arc::new(yapfast_core::chat::decode_picture(png.get_ref(), 128, 128).unwrap());
    let weak_picture = Arc::downgrade(&picture);
    a.app.state.pictures.insert(url.clone(), picture);
    a.app.state.profile = Some(ProfileView {
        profile: yapfast_core::chat::Profile {
            id: "200".into(),
            name: "Ada".into(),
            decoration: Some(decoration),
            ..Default::default()
        },
        large: false,
        loading: false,
        kept: false,
        note: None,
        platform_presence: None,
        board_open: false,
        activity_open: false,
        wishlist_open: false,
    });
    window.set_workspace(true);
    window.set_trusted(true);
    a.refresh(&window);
    assert_eq!(window.get_profile().decoration.size().width, 2);
    drop(a);
    window.show().unwrap();
    let inner = i_slint_core::window::WindowInner::from_pub(window.window());
    inner.ensure_tree_instantiated();
    let found =
        i_slint_core::item_tree::ItemRc::new_root(inner.component()).visit_descendants(|item| {
            if let Some(image) = item.downcast::<i_slint_core::items::ImageItem>() {
                let src = i_slint_core::items::ImageItem::FIELD_OFFSETS
                    .source()
                    .apply_pin(image.as_pin_ref())
                    .get();
                if src.size().width == 2 {
                    assert_eq!(
                        &src.to_rgba8().unwrap().as_bytes()[..4],
                        &[17, 231, 99, 255],
                        "actual native overlay uses decoded preset pixels"
                    );
                    assert_eq!(
                        src.to_rgba8().unwrap().as_bytes()[7],
                        0,
                        "native preset retains transparent corner"
                    );
                    let geometry = item.geometry();
                    assert!(
                        (geometry.size.width - 96.).abs() < 0.1,
                        "small-card decoration surrounds the production 80px avatar at 1.2x"
                    );
                    assert!(
                        geometry.size.width > 0. && geometry.size.height > 0.,
                        "native overlay has nonempty layout"
                    );
                    assert!(
                        (geometry.size.width - geometry.size.height).abs() < 0.1,
                        "surrounding preset retains square geometry"
                    );
                    return std::ops::ControlFlow::Break(());
                }
            }
            std::ops::ControlFlow::Continue(())
        });
    assert!(
        found.is_some(),
        "native card instantiates received decoration Image"
    );
    let mut a = owner.borrow_mut();
    a.app
        .state
        .profile
        .as_mut()
        .unwrap()
        .profile
        .decoration
        .as_mut()
        .unwrap()
        .expires_at = Some(1);
    a.refresh(&window);
    assert_eq!(window.get_profile().decoration.size().width, 0);
    a.app
        .state
        .profile
        .as_mut()
        .unwrap()
        .profile
        .decoration
        .as_mut()
        .unwrap()
        .expires_at = None;
    a.refresh(&window);
    assert_eq!(window.get_profile().decoration.size().width, 2);
    let mut retained_pixels = window.get_profile().decoration.to_rgba8().unwrap();
    a.app.dispatch(Action::CloseProfile);
    a.refresh(&window);
    assert_eq!(
        window.get_profile().decoration.size().width,
        0,
        "closed card does not retain decoration pixels in its window model"
    );
    // Exercise ordinary production cache pressure, not a test-only purge.
    for index in 0..401 {
        a.app.apply(Event::Picture {
            url: format!("owned-decoration-pressure-{index}"),
            picture: Arc::new(yapfast_core::chat::Picture::default()),
        });
    }
    a.refresh(&window);
    assert!(!a.app.state.pictures.contains_key(&url));
    assert!(
        weak_picture.upgrade().is_none(),
        "ordinary cache pressure releases old decoded Picture"
    );
    drop(a);
    inner.ensure_tree_instantiated();
    slint::platform::update_timers_and_animations();
    let bytes = retained_pixels.as_bytes().as_ptr();
    assert_eq!(
        retained_pixels.make_mut_bytes().as_ptr(),
        bytes,
        "closed native decoration releases every image-buffer owner without COW"
    );
}
