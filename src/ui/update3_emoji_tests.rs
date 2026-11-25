//! Imports the production picker; synthetic native accessibility/pointer/keyboard only.
use i_slint_core::{accessibility::AccessibleStringProperty, item_tree::ItemRc};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, ops::ControlFlow, rc::Rc};
slint::slint! {
    // Match production's uneditioned layout semantics. Edition 1.19 changes
    // conditional/repeated sizing and state defaults; that is a separate migration.
    #[edition = "1.18"]
    import { EmojiPicker, EmojiRow, EmojiCell, EmojiSection, EmojiTile, RailSection, SectionGlyph } from "../../ui/messages.slint";
    export { EmojiRow, EmojiCell, EmojiSection, EmojiTile, RailSection, SectionGlyph } from "../../ui/messages.slint";
    export component PickerHarness inherits Window {
        preferred-width: 500px; preferred-height: 400px;
        in-out property <[EmojiRow]> rows;
        in-out property <[EmojiSection]> sections;
        in-out property <[EmojiTile]> tiles;
        in-out property <[EmojiRow]> headings;
        in-out property <[RailSection]> rail;
        in-out property <length> grid-height;
        in-out property <length> viewport-top;
        in-out property <length> viewport-height;
        in-out property <bool> opened: true;
        in-out property <string> query;
        in-out property <EmojiCell> hover;
        callback picked(EmojiCell);
        if root.opened: EmojiPicker {
            emoji-rows: root.rows; emoji-sections: root.sections; emoji-query <=> root.query;
            emoji-tiles: root.tiles; emoji-headings: root.headings; grid-height: root.grid-height;
            rail-sections: root.rail;
            emoji-hover <=> root.hover; columns: 6; react-item: "yapfast:composer";
            scrolled-to(top, height) => { root.viewport-top = top; root.viewport-height = height; }
            pick(cell) => { root.picked(cell); }
        }
    }
}

fn sync_model<T: Clone + PartialEq + 'static>(model: ModelRc<T>, rows: Vec<T>) -> ModelRc<T> {
    if let Some(current) = model.as_any().downcast_ref::<VecModel<T>>() {
        // Keep native controls alive when a settle did not change their data.
        super::update_model(current, rows);
        model
    } else {
        ModelRc::new(VecModel::from(rows))
    }
}

fn publish_picker(w: &PickerHarness) {
    // The harness has no Adapter. Supply its presentation inputs using the
    // production geometry/range contract (emoji::visible_rows and emoji_grid):
    // visible rows plus two on either side, 8px padding, and 56px cells.
    let rows: Vec<_> = w.get_rows().iter().collect();
    let mut tiles = Vec::new();
    let mut headings = Vec::new();
    if w.get_opened() && !rows.is_empty() {
        let top = w.get_viewport_top();
        let height = if w.get_viewport_height() > 0.0 {
            w.get_viewport_height()
        } else {
            340.0
        };
        let first = rows
            .iter()
            .position(|row| row.top + row.height >= top)
            .unwrap_or(rows.len() - 1);
        let last = rows
            .iter()
            .rposition(|row| row.top <= top + height)
            .unwrap_or(first)
            .max(first);
        for row in &rows[first.saturating_sub(2)..=(last + 2).min(rows.len() - 1)] {
            for (column, cell) in row.cells.iter().enumerate() {
                tiles.push(EmojiTile {
                    cell,
                    x: 8.0 + column as f32 * crate::emoji::CELL,
                    y: row.top,
                    animated: false, // These still-image fixtures never request a decode.
                });
            }
            if !row.heading.is_empty() {
                headings.push(row.clone());
            }
        }
    }
    let rail = if w.get_opened() {
        w.get_sections()
            .iter()
            .map(|section| RailSection {
                name: section.icon.name.clone(),
                icon: section.icon,
                glyph: SectionGlyph::None, // The fixture is a picture-backed server.
                top: section.top,
                end: section.end,
            })
            .collect()
    } else {
        Vec::new()
    };
    w.set_grid_height(rows.last().map_or(0.0, |row| row.top + row.height));
    w.set_tiles(sync_model(w.get_tiles(), tiles));
    w.set_headings(sync_model(w.get_headings(), headings));
    w.set_rail(sync_model(w.get_rail(), rail));
}

fn settle(w: &PickerHarness) {
    publish_picker(w);
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    inner.ensure_tree_instantiated();
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::ZERO);
    slint::platform::update_timers_and_animations();
    // Instantiating/resizing the real picker reports its measured viewport.
    publish_picker(w);
    inner.ensure_tree_instantiated();
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
        icon: slint::Image::default(),
        glyph: SectionGlyph::None,
        top: 0.0,
        height: crate::emoji::ROW_HEIGHT,
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
        icon: slint::Image::default(),
        glyph: SectionGlyph::None,
        top: 0.0,
        height: crate::emoji::ROW_HEIGHT,
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
            icon: slint::Image::default(),
            glyph: SectionGlyph::None,
            top: 0.0,
            height: crate::emoji::ROW_HEIGHT,
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
        icon: slint::Image::default(),
        glyph: SectionGlyph::None,
        top: 0.0,
        height: crate::emoji::ROW_HEIGHT,
    }];
    scrolling.extend((0..30).map(|index| EmojiRow {
        heading: "Spacer".into(),
        cells: ModelRc::default(),
        icon: slint::Image::default(),
        glyph: SectionGlyph::None,
        top: crate::emoji::ROW_HEIGHT + index as f32 * crate::emoji::HEADING_HEIGHT,
        height: crate::emoji::HEADING_HEIGHT,
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
        w.get_viewport_top() > crate::emoji::ROW_HEIGHT + 2.0 * crate::emoji::HEADING_HEIGHT,
        "native scrolling must move beyond the hovered row and its overscan"
    );
    assert!(
        w.get_tiles()
            .iter()
            .all(|tile| tile.cell.key != "repair_art:90003"),
        "virtualized presentation must release the offscreen tile"
    );
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
            icon: slint::Image::default(),
            glyph: SectionGlyph::None,
            top: 0.0,
            height: crate::emoji::HEADING_HEIGHT,
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
    // This rail-only fixture still names a real heading row and a pixel range.
    w.set_rows(ModelRc::new(VecModel::from(vec![EmojiRow {
        heading: "REPAIR SERVER".into(),
        cells: ModelRc::default(),
        icon: slint::Image::from_rgba8(server.clone()),
        glyph: SectionGlyph::None,
        top: 0.0,
        height: crate::emoji::HEADING_HEIGHT,
    }])));
    w.set_sections(ModelRc::new(VecModel::from(vec![EmojiSection {
        heading: 0,
        icon_url: "".into(), // Pixels are supplied locally; no server request.
        top: 0.0,
        end: crate::emoji::HEADING_HEIGHT,
        icon: EmojiCell {
            key: "server:90002".into(),
            glyph: "Repair server".into(),
            name: "Repair server".into(),
            picture: slint::Image::from_rgba8(server.clone()),
            usable: true,
        },
    }])));
    drop(hover_control(&w, "Repair server"));
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
