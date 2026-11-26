//! Device-local, account-scoped groups for pinned direct messages.
use super::*;

const PREFIX: &str = "dm-group:";
const MAX_GROUPS: usize = 30;

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct Group {
    id: u64,
    name: String,
    rooms: Vec<String>,
    collapsed: bool,
}

impl Group {
    fn target(&self) -> String {
        format!("{PREFIX}{}", self.id)
    }
}

fn account(state: &AppState) -> Option<&str> {
    if state.preview {
        Some("preview")
    } else {
        state.discord_user.as_deref().or(state.account.as_deref())
    }
}

fn groups<'a>(layout: &'a Layout, state: &AppState) -> &'a [Group] {
    account(state)
        .and_then(|id| layout.dm_groups.get(id))
        .map_or(&[], Vec::as_slice)
}

pub(super) fn header(a: &Adapter, target: &str) -> bool {
    target.starts_with(PREFIX)
        && groups(&a.layout, &a.app.state)
            .iter()
            .any(|group| group.target() == target)
}

fn is_dm(a: &Adapter, target: &str) -> bool {
    account(&a.app.state).is_some()
        && a.app
            .state
            .rooms
            .iter()
            .any(|room| room.id == target && room.direct)
        && a.app.menu_choices(target).iter().any(|choice| {
            matches!(
                choice,
                yapfast_core::application::MenuChoice::Pin
                    | yapfast_core::application::MenuChoice::Unpin
            )
        })
}

#[cfg(test)]
#[test]
fn categories_cycle_between_all_pinned_dms_and_filtered_groups() {
    let mut layout = Layout::default();
    let mut state = AppState::default();
    state.preview = true;
    layout.dm_groups.insert(
        "preview".into(),
        vec![Group {
            id: 1,
            name: "Friends".into(),
            rooms: vec!["b".into()],
            collapsed: true,
        }],
    );
    let layout: Layout = serde_json::from_slice(&serde_json::to_vec(&layout).unwrap()).unwrap();
    let rooms = vec![
        RoomRow {
            id: "b".into(),
            pinned: true,
            heading: "PINNED DMS".into(),
            ..Default::default()
        },
        RoomRow {
            id: "c".into(),
            pinned: true,
            ..Default::default()
        },
        RoomRow {
            id: "d".into(),
            ..Default::default()
        },
    ];
    let collapsed = arrange(&layout, &state, rooms.clone(), false);
    assert_eq!(collapsed[0].heading, "Friends");
    assert!(collapsed[0].hidden && collapsed[0].collapsed);
    assert_eq!(collapsed[1].heading, "PINNED DMS");
    assert_eq!(
        collapsed.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["dm-group:1", "c", "d"]
    );
    let filtered = arrange(&layout, &state, rooms.clone(), true);
    assert!(!filtered[0].collapsed);
    assert_eq!(filtered[1].id, "b");
    assert!(filtered[1].heading.is_empty());
    state.preview = false;
    assert_eq!(
        arrange(&layout, &state, rooms, false)[0].heading,
        "PINNED DMS"
    );
}

fn row(id: &str, text: &str) -> MenuRow {
    MenuRow {
        id: id.into(),
        text: text.into(),
        plain: true,
        ..Default::default()
    }
}

pub(super) fn heading_menu(a: &Adapter, target: &str) -> Option<menus::Menu> {
    let groups = groups(&a.layout, &a.app.state);
    let index = groups.iter().position(|g| g.target() == target)?;
    let mut rows = vec![row("dm-group:rename", "Rename category")];
    if index > 0 {
        rows.push(row("dm-group:up", "Move category up"));
    }
    if index + 1 < groups.len() {
        rows.push(row("dm-group:down", "Move category down"));
    }
    rows.push(row("dm-group:delete", "Remove category"));
    Some((rows, vec![]))
}

pub(super) fn room_menu(a: &Adapter, id: &str, person: bool) -> Option<(MenuRow, Vec<MenuRow>)> {
    if person || !is_dm(a, id) {
        return None;
    }
    let mut items = vec![row("dm-group:default", "Pinned DMs")];
    let groups = groups(&a.layout, &a.app.state);
    items.extend(groups.iter().map(|group| row(&group.target(), &group.name)));
    if groups.len() < MAX_GROUPS {
        items.push(row("dm-group:new", "New category…"));
    }
    Some((row("dm-group:menu", "Pin to category"), items))
}

pub(super) fn choose(a: &mut Adapter, window: &YapfastWindow, target: &str, id: &str) -> bool {
    if !id.starts_with(PREFIX) {
        return false;
    }
    let Some(owner) = account(&a.app.state).map(str::to_owned) else {
        return true;
    };
    if id == "dm-group:new" && is_dm(a, target) {
        prompt(window, &owner, target, "");
    } else if header(a, target) {
        let groups = a.layout.dm_groups.get_mut(&owner).unwrap();
        let index = groups.iter().position(|g| g.target() == target).unwrap();
        match id {
            "dm-group:rename" => prompt(window, &owner, target, &groups[index].name),
            "dm-group:delete" => {
                groups.remove(index);
                save(a, window);
            }
            "dm-group:up" if index > 0 => {
                groups.swap(index, index - 1);
                save(a, window);
            }
            "dm-group:down" if index + 1 < groups.len() => {
                groups.swap(index, index + 1);
                save(a, window);
            }
            _ => {}
        }
    } else if is_dm(a, target) && (id == "dm-group:default" || header(a, id)) {
        assign(a, window, &owner, target, id);
    }
    true
}

fn assign(a: &mut Adapter, window: &YapfastWindow, owner: &str, room: &str, target: &str) {
    let groups = a.layout.dm_groups.entry(owner.to_owned()).or_default();
    for group in groups {
        group.rooms.retain(|id| id != room);
        if group.target() == target {
            group.rooms.push(room.to_owned());
        }
    }
    a.action(
        window,
        Action::Pin {
            room: room.into(),
            pinned: true,
        },
    );
    save_pins(a);
    a.sidebar = None;
    a.show_sidebar(window);
}

pub(super) fn save_pins(a: &mut Adapter) {
    a.layout.pinned = a.app.state.pinned.clone();
    if let Some(groups) = account(&a.app.state).and_then(|id| a.layout.dm_groups.get_mut(id)) {
        for group in groups {
            group.rooms.retain(|id| a.layout.pinned.contains(id));
        }
    }
    a.layout.write(&a.root);
}

fn save(a: &mut Adapter, window: &YapfastWindow) {
    a.layout.write(&a.root);
    a.sidebar = None;
    a.show_sidebar(window);
}

pub(super) fn toggle(a: &mut Adapter, window: &YapfastWindow, target: &str) {
    if let Some(group) = account(&a.app.state)
        .and_then(|id| a.layout.dm_groups.get_mut(id))
        .and_then(|groups| groups.iter_mut().find(|group| group.target() == target))
    {
        group.collapsed = !group.collapsed;
        save(a, window);
    }
}

pub(super) fn arrange(
    layout: &Layout,
    state: &AppState,
    rows: Vec<RoomRow>,
    filtering: bool,
) -> Vec<RoomRow> {
    let groups = groups(layout, state);
    if groups.is_empty() {
        return rows;
    }
    let mut result = Vec::with_capacity(rows.len() + groups.len());
    let mut rows: Vec<_> = rows.into_iter().map(Some).collect();
    for group in groups {
        let mut children = Vec::new();
        for slot in &mut rows {
            if slot
                .as_ref()
                .is_some_and(|r| r.pinned && group.rooms.iter().any(|id| id == r.id.as_str()))
            {
                let mut child = slot.take().unwrap();
                child.heading = Default::default();
                children.push(child);
            }
        }
        if filtering && children.is_empty() {
            continue;
        }
        let target: SharedString = group.target().into();
        let collapsed = group.collapsed && !filtering;
        result.push(RoomRow {
            id: target.clone(),
            category: target,
            heading: group.name.as_str().into(),
            collapsed,
            hidden: true,
            direct: true,
            ..Default::default()
        });
        if !collapsed {
            result.extend(children);
        }
    }
    let mut first_pin = true;
    for mut row in rows.into_iter().flatten() {
        if row.pinned {
            row.heading = if first_pin {
                "PINNED DMS".into()
            } else {
                Default::default()
            };
            first_pin = false;
        }
        result.push(row);
    }
    result
}

fn prompt(window: &YapfastWindow, owner: &str, target: &str, name: &str) {
    let view = window.global::<DmCategories>();
    view.set_owner(owner.into());
    view.set_target(target.into());
    view.set_renaming(target.starts_with(PREFIX));
    view.set_name(name.into());
    view.set_error(Default::default());
    view.set_open(true);
}

pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    let weak = Rc::downgrade(adapter);
    let window_weak = window.as_weak();
    window.global::<DmCategories>().on_save(move || {
        let (Some(adapter), Some(window)) = (weak.upgrade(), window_weak.upgrade()) else {
            return;
        };
        let mut a = adapter.borrow_mut();
        let view = window.global::<DmCategories>();
        let owner = view.get_owner();
        let target = view.get_target();
        if !view.get_open() || account(&a.app.state) != Some(owner.as_str()) {
            view.set_open(false);
            return;
        }
        let text = view.get_name();
        let name = text.trim();
        if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
            view.set_error("Enter a category name up to 40 characters.".into());
            return;
        }
        if view.get_renaming() {
            if let Some(group) = a
                .layout
                .dm_groups
                .get_mut(owner.as_str())
                .and_then(|groups| {
                    groups
                        .iter_mut()
                        .find(|group| group.target() == target.as_str())
                })
            {
                group.name = name.to_owned();
                save(&mut a, &window);
            }
        } else if is_dm(&a, &target) {
            let groups = a.layout.dm_groups.entry(owner.to_string()).or_default();
            if groups.len() < MAX_GROUPS {
                let id = groups
                    .iter()
                    .map(|group| group.id)
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                let group = Group {
                    id,
                    name: name.to_owned(),
                    rooms: vec![],
                    collapsed: false,
                };
                let category = group.target();
                groups.push(group);
                assign(&mut a, &window, &owner, &target, &category);
            }
        }
        view.set_open(false);
    });
}
