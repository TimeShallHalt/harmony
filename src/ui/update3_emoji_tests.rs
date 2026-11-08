//! Imports the production picker; synthetic native accessibility/pointer/keyboard only.
use i_slint_core::{accessibility::AccessibleStringProperty, item_tree::ItemRc};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, ops::ControlFlow, rc::Rc};
slint::slint! {
    import { EmojiPicker, EmojiRow, EmojiCell, EmojiSection } from "../../ui/messages.slint";
    export { EmojiRow, EmojiCell, EmojiSection } from "../../ui/messages.slint";
    export component PickerHarness inherits Window {
        preferred-width: 500px; preferred-height: 400px;
        in-out property <[EmojiRow]> rows;
        in-out property <[EmojiSection]> sections;
        in-out property <bool> opened: true;
        in-out property <string> query;
        in-out property <EmojiCell> hover;
        callback picked(EmojiCell);
        if root.opened: EmojiPicker {
            emoji-rows: root.rows; emoji-sections: root.sections; emoji-query <=> root.query;
            emoji-hover <=> root.hover; columns: 6; react-item: "yapfast:composer";
            pick(cell) => { root.picked(cell); }
        }
    }
}
fn settle(w: &PickerHarness) {
    i_slint_core::window::WindowInner::from_pub(w.window()).ensure_tree_instantiated();
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::ZERO);
    slint::platform::update_timers_and_animations();
}
#[test]
fn picker_native_hover_is_metadata_only_and_empty_search_clears_selection() {
    i_slint_backend_testing::init_no_event_loop();
    let w = PickerHarness::new().unwrap();
    let cell = EmojiCell {
        key: "fixture".into(),
        name: "fixture emoji".into(),
        glyph: "😀".into(),
        usable: true,
        picture: slint::Image::from_rgba8(slint::SharedPixelBuffer::new(36, 36)),
    };
    let cells = Rc::new(VecModel::from(vec![cell]));
    let weak = Rc::downgrade(&cells);
    w.set_rows(ModelRc::new(VecModel::from(vec![EmojiRow {
        heading: "".into(),
        cells: ModelRc::from(cells.clone()),
    }])));
    drop(cells);
    w.show().unwrap();
    settle(&w);
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    let item = ItemRc::new_root(inner.component())
        .visit_descendants(|item| {
            if item.is_visible()
                && item
                    .accessible_string_property(AccessibleStringProperty::Label)
                    .as_deref()
                    == Some("Insert fixture emoji")
            {
                ControlFlow::Break(item.clone())
            } else {
                ControlFlow::Continue(())
            }
        })
        .expect("native accessible picker tile");
    let p = item.map_to_window(Default::default());
    let g = item.geometry();
    w.window()
        .dispatch_event(slint::platform::WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(p.x + g.width() / 2., p.y + g.height() / 2.),
        });
    settle(&w);
    assert_eq!(
        w.get_hover().key,
        "fixture",
        "real pointer hover populated footer metadata"
    );
    assert_eq!(
        w.get_hover().picture.size().width,
        0,
        "footer metadata must not pin the tile's bitmap"
    );
    let picked = Rc::new(RefCell::new(Vec::new()));
    let out = picked.clone();
    w.on_picked(move |cell| out.borrow_mut().push(cell.key.to_string()));
    item.accessible_action(&i_slint_core::accessibility::AccessibilityAction::Default);
    assert_eq!(&*picked.borrow(), &["fixture"]);
    drop(item);
    w.set_query("no-match-fixture".into());
    w.set_rows(ModelRc::new(VecModel::<EmojiRow>::default()));
    settle(&w);
    assert!(
        w.get_hover().key.is_empty(),
        "no-match search clears stale footer"
    );
    assert!(
        weak.upgrade().is_none(),
        "empty picker releases former cell model"
    );
    assert_eq!(w.get_rows().row_count(), 0);
}

fn hover_control(w: &PickerHarness, label: &str) -> ItemRc {
    settle(w);
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    let item = ItemRc::new_root(inner.component())
        .visit_descendants(|item| {
            if item.is_visible()
                && item
                    .accessible_string_property(AccessibleStringProperty::Label)
                    .as_deref()
                    == Some(label)
            {
                ControlFlow::Break(item.clone())
            } else {
                ControlFlow::Continue(())
            }
        })
        .expect("production accessible picker control");
    let p = item.map_to_window(Default::default());
    let g = item.geometry();
    w.window()
        .dispatch_event(slint::platform::WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(p.x + g.width() / 2., p.y + g.height() / 2.),
        });
    settle(w);
    item
}

fn footer_picture(w: &PickerHarness) -> slint::Image {
    settle(w);
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    ItemRc::new_root(inner.component())
        .visit_descendants(|item| {
            if item.is_visible() && item.map_to_window(Default::default()).y >= 352. {
                if let Some(image) = item.downcast::<i_slint_core::items::ImageItem>() {
                    return ControlFlow::Break(
                        i_slint_core::items::ImageItem::FIELD_OFFSETS
                            .source()
                            .apply_pin(image.as_pin_ref())
                            .get(),
                    );
                }
            }
            ControlFlow::Continue(())
        })
        .unwrap_or_default()
}

fn unique_pixels(buffer: &mut slint::SharedPixelBuffer<slint::Rgba8Pixel>) {
    let original = buffer.as_bytes().as_ptr();
    assert_eq!(
        buffer.make_mut_bytes().as_ptr(),
        original,
        "cleared picker must release every shared image buffer (no COW copy)"
    );
}

#[test]
fn custom_and_server_hover_resolve_current_art_without_retaining_removed_resources() {
    i_slint_backend_testing::init_no_event_loop();
    let w = PickerHarness::new().unwrap();
    // Deliberately no inline-font registration: an ordinary custom picker glyph is its name.
    let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(36, 36);
    pixels
        .make_mut_slice()
        .fill(slint::Rgba8Pixel::new(217, 29, 43, 255));
    let cell = EmojiCell {
        key: "repair_art:90001".into(),
        glyph: ":repair_art:".into(),
        name: ":repair_art:".into(),
        picture: slint::Image::from_rgba8(pixels.clone()),
        usable: true,
    };
    assert_eq!(cell.glyph, cell.name);
    let cells = Rc::new(VecModel::from(vec![cell]));
    let weak = Rc::downgrade(&cells);
    let rows = Rc::new(VecModel::from(vec![EmojiRow {
        heading: "".into(),
        cells: cells.clone().into(),
    }]));
    w.set_rows(rows.clone().into());
    drop(cells);
    w.show().unwrap();
    let item = hover_control(&w, "Insert :repair_art:");
    assert_eq!(w.get_hover().key, "repair_art:90001");
    assert_eq!(
        w.get_hover().picture.size().width,
        0,
        "hover remains metadata only"
    );
    assert_eq!(
        footer_picture(&w)
            .to_rgba8()
            .map(|b| b.as_bytes()[..4].to_vec()),
        Some(vec![217, 29, 43, 255]),
        "custom footer must display loaded identity artwork, not its placeholder"
    );
    let picked = Rc::new(RefCell::new(Vec::new()));
    let out = picked.clone();
    w.on_picked(move |cell| out.borrow_mut().push(cell.key.to_string()));
    item.accessible_action(&i_slint_core::accessibility::AccessibilityAction::Default);
    assert_eq!(&*picked.borrow(), &["repair_art:90001"]);
    drop(item);
    // Same row count and same key, but a rebuilt source has different pixels.
    let mut replacement = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(36, 36);
    replacement
        .make_mut_slice()
        .fill(slint::Rgba8Pixel::new(19, 181, 71, 255));
    rows.set_row_data(
        0,
        EmojiRow {
            heading: "".into(),
            cells: ModelRc::new(VecModel::from(vec![EmojiCell {
                key: "repair_art:90001".into(),
                glyph: ":repair_art:".into(),
                name: ":repair_art:".into(),
                picture: slint::Image::from_rgba8(replacement.clone()),
                usable: true,
            }])),
        },
    );
    settle(&w);
    assert!(
        weak.upgrade().is_none(),
        "rebuild releases the old cell model"
    );
    assert_eq!(
        footer_picture(&w)
            .to_rgba8()
            .map(|b| b.as_bytes()[..4].to_vec()),
        Some(vec![19, 181, 71, 255]),
        "same-size rebuild resolves current identity pixels"
    );
    unique_pixels(&mut pixels);
    w.set_query("no-match-repair".into());
    rows.set_vec(Vec::new());
    settle(&w);
    assert!(w.get_hover().key.is_empty());
    assert_eq!(footer_picture(&w).size().width, 0);
    unique_pixels(&mut replacement);
    // Scroll the actual native grid off the hovered row, then let its source go.
    let offscreen = Rc::new(VecModel::from(vec![EmojiCell {
        key: "repair_art:90003".into(),
        glyph: ":offscreen:".into(),
        name: ":offscreen:".into(),
        picture: slint::Image::from_rgba8(replacement.clone()),
        usable: true,
    }]));
    let offscreen_weak = Rc::downgrade(&offscreen);
    let mut scrolling = vec![EmojiRow {
        heading: "".into(),
        cells: offscreen.clone().into(),
    }];
    scrolling.extend((0..30).map(|_| EmojiRow {
        heading: "Spacer".into(),
        cells: ModelRc::default(),
    }));
    w.set_query("".into());
    rows.set_vec(scrolling);
    drop(offscreen);
    drop(hover_control(&w, "Insert :offscreen:"));
    assert_eq!(footer_picture(&w).size().width, 36);
    w.window()
        .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(200., 150.),
            delta_x: 0.,
            delta_y: -900.,
        });
    settle(&w);
    assert!(
        w.get_hover().key.is_empty(),
        "scrolling offscreen releases the footer identity"
    );
    assert_eq!(footer_picture(&w).size().width, 0);
    rows.set_row_data(
        0,
        EmojiRow {
            heading: "Gone".into(),
            cells: ModelRc::default(),
        },
    );
    settle(&w);
    assert!(offscreen_weak.upgrade().is_none());
    unique_pixels(&mut replacement);
    rows.set_vec(Vec::new());
    // A picture-backed server section also uses identity resolution, not a font glyph.
    let mut server = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(24, 24);
    server
        .make_mut_slice()
        .fill(slint::Rgba8Pixel::new(31, 61, 211, 255));
    w.set_query("".into());
    w.set_sections(ModelRc::new(VecModel::from(vec![EmojiSection {
        row: 0,
        end: 1,
        icon: EmojiCell {
            key: "server:90002".into(),
            glyph: "Repair server".into(),
            name: "Repair server".into(),
            picture: slint::Image::from_rgba8(server.clone()),
            usable: true,
        },
    }])));
    drop(hover_control(&w, "Jump to Repair server"));
    assert_eq!(
        footer_picture(&w)
            .to_rgba8()
            .map(|b| b.as_bytes()[..4].to_vec()),
        Some(vec![31, 61, 211, 255])
    );
    w.set_opened(false);
    w.set_sections(ModelRc::default());
    w.set_rows(ModelRc::default());
    settle(&w);
    unique_pixels(&mut server);
    w.set_opened(true);
    settle(&w);
    assert_eq!(
        footer_picture(&w).size().width,
        0,
        "reopen cannot revive removed artwork"
    );
}
