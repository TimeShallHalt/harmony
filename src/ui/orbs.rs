use super::*;

pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    let (weak, win) = (Rc::downgrade(adapter), window.as_weak());
    window.on_orbs_page(move |open| {
        let (Some(adapter), Some(w)) = (weak.upgrade(), win.upgrade()) else {
            return;
        };
        let mut a = adapter.borrow_mut();
        #[cfg(feature = "discord")]
        a.action(
            &w,
            if open {
                Action::ReadOrbs
            } else {
                Action::CloseOrbs
            },
        );
        #[cfg(not(feature = "discord"))]
        {
            let _ = open;
            a.show_orbs(&w);
        }
    });
}

impl Adapter {
    pub(super) fn show_orbs(&mut self, window: &YapfastWindow) {
        let ui = window.global::<Orbs>();
        if !window.get_settings_open() || window.get_settings_section() != 12 {
            #[cfg(feature = "discord")]
            if self.orbs_shown.take().is_some() {
                ui.set_items(Default::default());
                ui.set_quests(Default::default());
                ui.set_balance(Default::default());
                ui.set_quest_note(Default::default());
                ui.set_shop_note(Default::default());
                ui.set_can_refresh(false);
            }
            return;
        }
        #[cfg(feature = "discord")]
        {
            let state = &self.app.state;
            let online = state.discord_online && !state.preview && state.discord_user.is_some();
            let page = state
                .orbs
                .as_ref()
                .filter(|page| online && state.discord_user.as_deref() == Some(&*page.request));
            let report = page.and_then(|p| p.report.as_ref());
            ui.set_can_refresh(online && (page.is_none() || report.is_some()));
            if let Some(report) = report {
                if self
                    .orbs_shown
                    .as_ref()
                    .is_some_and(|r| Arc::ptr_eq(r, report))
                {
                    return;
                }
                ui.set_balance(report.balance.map_or_else(
                    || "Orbs balance unavailable".into(),
                    |b| format!("{b} Orbs").into(),
                ));
                let mut notes = Vec::new();
                if !report.current_available {
                    notes.push("Current Quests unavailable.".to_owned());
                }
                if !report.history_available {
                    notes.push("Claim history unavailable.".to_owned());
                }
                if report.current_available && report.quests.as_ref().is_some_and(Vec::is_empty) {
                    notes.push("Discord returned no Quests.".into());
                }
                if let Some(until) = &report.enrollment_blocked_until {
                    notes.push(format!("New enrollments open {until}."));
                }
                ui.set_quest_note(notes.join(" ").into());
                ui.set_quests(ModelRc::new(VecModel::from(
                    report
                        .quests
                        .iter()
                        .flatten()
                        .map(|q| {
                            let mut details = q.progress.clone();
                            if let Some(expires) = &q.expires {
                                details.push(format!("Ends {expires}"));
                            }
                            if let Some(until) = &q.claim_by {
                                details.push(format!("Claim deadline {until}"));
                            }
                            if !q.reward.is_empty() {
                                details.push(format!("Reward · {}", q.reward));
                            }
                            if let Some(orbs) = q.claimed_orbs {
                                details.push(format!("Claimed {orbs} Orbs"));
                            }
                            QuestRow {
                                id: q.id.to_string().into(),
                                name: q.name.as_str().into(),
                                state: q.status.into(),
                                details: details.join(" · ").into(),
                            }
                        })
                        .collect::<Vec<_>>(),
                )));
                ui.set_shop_note(
                    match &report.items {
                        None => "Shop catalog unavailable.",
                        Some(items) if items.is_empty() => {
                            "Discord returned no items with an Orbs price."
                        }
                        _ => "",
                    }
                    .into(),
                );
                ui.set_items(ModelRc::new(VecModel::from(
                    report
                        .items
                        .iter()
                        .flatten()
                        .map(|item| OrbItemRow {
                            id: item.id.to_string().into(),
                            name: item.name.as_str().into(),
                            summary: item.summary.as_str().into(),
                            price: item.price.map_or_else(
                                || "Price in Discord".into(),
                                |price| format!("{price} Orbs").into(),
                            ),
                            ownership: match item.owned {
                                Some(true) => "In your collection",
                                Some(false) => "",
                                None => "Check ownership in Discord",
                            }
                            .into(),
                        })
                        .collect::<Vec<_>>(),
                )));
                self.orbs_shown = Some(report.clone());
                return;
            }
            if self.orbs_shown.take().is_some() {
                ui.set_items(Default::default());
                ui.set_quests(Default::default());
            }
            if page.is_some() {
                ui.set_balance("Checking balance…".into());
                ui.set_quest_note("Loading Quests…".into());
                ui.set_shop_note("Loading Shop…".into());
                return;
            }
        }
        ui.set_balance("Orbs balance unavailable".into());
        ui.set_quest_note(
            "Open Quest Home to check eligibility, opt in, and claim rewards with Discord.".into(),
        );
        ui.set_shop_note(Default::default());
    }
}

#[cfg(all(test, feature = "discord"))]
mod tests {
    use super::*;
    use i_slint_core::accessibility::{AccessibilityAction, AccessibleStringProperty};
    use i_slint_core::item_tree::ItemRc;
    use yapfast_core::chat::DiscordCommand;
    use yapfast_core::yapfast_discord::orbs::{Item, Overview};

    fn press(w: &YapfastWindow, label: &str) {
        i_slint_core::properties::ChangeTracker::run_change_handlers();
        let inner = i_slint_core::window::WindowInner::from_pub(w.window());
        inner.ensure_tree_instantiated();
        let control = ItemRc::new_root(inner.component())
            .visit_descendants(|item| {
                if item
                    .accessible_string_property(AccessibleStringProperty::Label)
                    .as_deref()
                    == Some(label)
                {
                    std::ops::ControlFlow::Break(item.clone())
                } else {
                    std::ops::ControlFlow::Continue(())
                }
            })
            .unwrap_or_else(|| panic!("missing control {label}"));
        control.try_scroll_into_visible(i_slint_core::items::ScrollBehavior::Instant);
        assert!(control.is_visible(), "scrolling did not reveal {label}");
        control.accessible_action(&AccessibilityAction::Default);
        i_slint_core::properties::ChangeTracker::run_change_handlers();
    }

    #[test]
    fn update7_settings_navigation_and_hosted_checkout_reconcile_without_spending() {
        i_slint_backend_testing::init_no_event_loop();
        let dir = tempfile::tempdir().unwrap();
        let w = YapfastWindow::new().unwrap();
        let adapter = Adapter::new(
            Arc::new(BackgroundRuntime::new().unwrap()),
            dir.path().into(),
            &w,
        )
        .unwrap();
        bind_callbacks(&w, &adapter);
        {
            let mut a = adapter.borrow_mut();
            a.app.dispatch(Action::Preview);
            a.refresh(&w);
        }
        w.window().set_size(slint::LogicalSize::new(1280., 860.));
        w.show().unwrap();
        w.set_settings_open(true);
        w.set_settings_section(0);
        i_slint_core::properties::ChangeTracker::run_change_handlers();
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(500));
        slint::platform::update_timers_and_animations();
        press(&w, "Local Profile");
        assert!(w.global::<LocalProfile>().get_available());
        w.global::<LocalProfile>()
            .invoke_edit_color(true, "#448899".into());
        press(&w, "Save on this device");
        assert!(dir.path().join("local-profiles/preview.json").is_file());
        press(&w, "Quests & Orbs");
        let ui = w.global::<Orbs>();
        assert!(!ui.get_can_refresh());
        assert!(ui.get_balance().contains("unavailable"));
        let link = Rc::new(RefCell::new(String::new()));
        let record = link.clone();
        w.on_open_link(move |url| *record.borrow_mut() = url.to_string());
        press(&w, "Open Discord Quest Home");
        assert_eq!(*link.borrow(), "https://discord.com/discovery/quests");
        let mut a = adapter.borrow_mut();
        a.app.state.preview = false;
        a.app.state.discord_online = true;
        a.app.state.discord_user = Some("42".into());
        let read = |a: &mut Adapter| match a.app.dispatch(Action::ReadOrbs).pop().unwrap() {
            Effect::Backend(Command::Discord(DiscordCommand::ReadOrbs(r))) => r,
            _ => panic!("expected account read"),
        };
        let stale = read(&mut a);
        let request = read(&mut a);
        let report = Arc::new(Overview {
            balance: Some(200),
            items: Some(vec![Item {
                id: 903,
                name: "Night sky".into(),
                summary: "Starry decoration".into(),
                price: Some(150),
                owned: Some(false),
            }]),
            ..Default::default()
        });
        a.app.apply(Event::Orbs {
            request: stale,
            report: report.clone(),
        });
        assert!(a.app.state.orbs.as_ref().unwrap().report.is_none());
        a.app.apply(Event::Orbs { request, report });
        a.show_orbs(&w);
        assert_eq!(ui.get_balance(), "200 Orbs");
        let outbound = a.backend_command_count;
        drop(a);
        press(&w, "Night sky · 150 Orbs");
        press(&w, "Review in Discord Shop");
        assert_eq!(*link.borrow(), "https://discord.com/shop#itemSkuId=903");
        assert_eq!(ui.get_balance(), "200 Orbs");
        assert_eq!(adapter.borrow().backend_command_count, outbound);
        // A focus return issues a new read. This fixture has no connected
        // backend, so it must clear the previous price and balance.
        w.set_window_focused(false);
        i_slint_core::properties::ChangeTracker::run_change_handlers();
        w.set_window_focused(true);
        i_slint_core::properties::ChangeTracker::run_change_handlers();
        assert!(ui.get_balance().contains("unavailable"));
        assert_eq!(ui.get_items().row_count(), 0);
        assert_eq!(adapter.borrow().backend_command_count, outbound + 1);
    }
}
