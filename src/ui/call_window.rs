use super::*;

pub(super) struct Popout {
    pub view: CallWindow,
    room: String,
    pub tiles: Option<Tiles>,
}

pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    let weak = Rc::downgrade(adapter);
    window.on_pop_out_call(move || {
        if let Some(adapter) = weak.upgrade() {
            let mut adapter = adapter.borrow_mut();
            if let Some(window) = adapter.window.upgrade()
                && let Err(error) = adapter.open_call_window(&window)
            {
                window.set_notice(format!("Could not open the call window. {error}").into());
            }
        }
    });
    let weak = Rc::downgrade(adapter);
    window.on_popout_share_sources_ready(move || {
        if let Some(adapter) = weak.upgrade() {
            let adapter = adapter.borrow();
            if let (Some(popout), Some(window)) = (&adapter.call_window, adapter.window.upgrade()) {
                let view = &popout.view;
                view.set_screens(window.get_share_screens());
                view.set_windows(window.get_share_windows());
                view.set_cameras(window.get_share_cameras());
                view.set_portal(!window.get_share_lists());
                view.set_max_resolution(window.get_share_max_resolution());
                view.set_fps_720(window.get_share_fps_720());
                view.set_fps_hd(window.get_share_fps_hd());
                view.set_resolution(window.get_share_resolution());
                view.set_size_choice(window.get_share_size());
                view.set_fps(window.get_share_fps().to_string().into());
                let bitrate = window.get_share_bitrate();
                view.set_bitrate(if bitrate == 0 {
                    "".into()
                } else {
                    bitrate.to_string().into()
                });
                view.set_high_quality(window.get_share_high_quality());
                view.set_cursor(window.get_share_cursor());
                view.invoke_show_picker();
            }
        }
    });
}

impl Adapter {
    fn open_call_window(&mut self, window: &YapfastWindow) -> Result<()> {
        let voice = &self.app.state.voice;
        let Some(room) = voice.room.clone().filter(|_| voice.active()) else {
            return Ok(());
        };
        if let Some(popout) = &self.call_window {
            popout.view.window().set_minimized(false);
            popout
                .view
                .window()
                .with_winit_window(|native| native.focus_window());
            return Ok(());
        }
        let view = CallWindow::new()?;
        let update_input = self.updates.input_clock();
        view.window().on_winit_window_event(move |_, event| {
            release_updates::record_input(&update_input, event);
            slint::winit_030::EventResult::Propagate
        });
        let weak = self.weak.clone();
        let main = move || weak.upgrade()?.borrow().window.upgrade();
        macro_rules! forward {
            ($bind:ident, $invoke:ident $(, $arg:ident)*) => {
                let main = main.clone();
                view.$bind(move |$($arg),*| {
                    if let Some(window) = main() { window.$invoke($($arg),*); }
                });
            };
        }
        forward!(on_toggle_mute, invoke_toggle_mute);
        forward!(on_toggle_deafen, invoke_toggle_deafen);
        forward!(on_toggle_camera, invoke_toggle_camera);
        forward!(on_toggle_share, invoke_toggle_share);
        forward!(on_leave, invoke_leave_voice);
        forward!(on_focus_tile, invoke_focus_tile, slot);
        forward!(on_watch_stream, invoke_watch_stream, id);
        forward!(on_stop_watching, invoke_stop_watching);
        forward!(
            on_go_live,
            invoke_go_live,
            source,
            resolution,
            size,
            fps,
            bitrate,
            effort,
            cursor
        );
        view.on_parse_whole(|text| yapfast_core::volume::parse_percentage(&text).unwrap_or(-1));
        view.on_parse_size(|text| callbacks::share_size(text.as_str()));
        let main_for_sources = main.clone();
        view.on_list_sources(move || {
            if let Some(window) = main_for_sources() {
                window.set_share_picker_in_call_window(true);
                window.invoke_list_share_sources();
            }
        });

        let weak = self.weak.clone();
        view.on_tiles(
            move |columns,
                  width,
                  height,
                  gap,
                  top,
                  scrolled,
                  shown,
                  focused,
                  focus_width,
                  focus_height| {
                if let Some(adapter) = weak.upgrade()
                    && let Ok(mut a) = adapter.try_borrow_mut()
                    && let Some(popout) = a.call_window.as_mut()
                {
                    popout.tiles = Some(Tiles {
                        columns: columns.max(0) as u32,
                        width,
                        height,
                        gap,
                        top,
                        scrolled,
                        shown,
                        focused: usize::try_from(focused).ok(),
                        focus_width,
                        focus_height,
                    });
                    if let Some(window) = a.window.upgrade() {
                        callbacks::place_videos(&a, &window);
                    }
                }
            },
        );
        let weak = self.weak.clone();
        let close = move |reopen: bool| {
            let weak = weak.clone();
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let Some(adapter) = weak.upgrade() {
                    let mut a = adapter.borrow_mut();
                    if let Some(main) = a.window.upgrade() {
                        a.close_call_window(&main);
                    }
                    drop(a);
                    if reopen {
                        shell::return_to_call();
                    }
                }
            });
        };
        let return_call = close.clone();
        view.on_return_call(move || return_call(true));
        view.window().on_close_requested(move || {
            close(false);
            slint::CloseRequestResponse::KeepWindowShown
        });
        self.call_window = Some(Popout {
            view,
            room,
            tiles: None,
        });
        window.invoke_exit_call_fullscreen();
        self.sync_call_window(window);
        let popout = self.call_window.as_ref().expect("opening an active call");
        if let Err(error) = popout.view.show() {
            self.close_call_window(window);
            return Err(error.into());
        }
        window.set_call_popped(true);
        Ok(())
    }

    pub(super) fn close_call_window(&mut self, window: &YapfastWindow) {
        if let Some(popout) = self.call_window.take()
            && let Err(error) = popout.view.hide()
        {
            tracing::warn!(%error, "could not hide the call window");
        }
        window.set_share_picker_in_call_window(false);
        window.set_call_popped(false);
        callbacks::place_videos(self, window);
    }

    pub(super) fn sync_call_window(&mut self, window: &YapfastWindow) {
        let Some(popout) = &self.call_window else {
            return;
        };
        let voice = &self.app.state.voice;
        if !voice.active() || voice.room.as_deref() != Some(popout.room.as_str()) {
            self.close_call_window(window);
            return;
        }
        window.set_call_popped(true);
        let view = &popout.view;
        theme::Theme::shown(window).set(&view.global::<Theme>());
        macro_rules! mirror {
            ($($to:ident: $from:ident),* $(,)?) => { $(view.$to(window.$from());)* };
        }
        mirror!(
            set_call_name: get_voice_name, set_status: get_voice_status,
            set_participants: get_participants, set_frames: get_participant_frames,
            set_camera_starting: get_camera_starting, set_camera_on: get_camera_on, set_can_film: get_can_film,
            set_muted: get_muted, set_deafened: get_deafened, set_ready: get_voice_ready,
            set_leaving: get_voice_leaving, set_can_share: get_can_share, set_sharing: get_sharing,
            set_share_live: get_share_live, set_share_status: get_share_status,
            set_share_viewers: get_share_viewers, set_share_viewer_names: get_share_viewer_names,
            set_stream_tiles: get_stream_tiles, set_watch_user: get_watch_user,
            set_watch_status: get_watch_status, set_watch_viewers: get_watch_viewers,
            set_watch_viewer_names: get_watch_viewer_names, set_focused_tile: get_call_focus,
        );
        self.sync_call_frames(window);
    }

    pub(super) fn sync_call_frames(&self, window: &YapfastWindow) {
        if let Some(popout) = &self.call_window {
            popout.view.set_share_frame(window.get_share_frame());
            popout.view.set_watch_frame(window.get_watch_frame());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_the_call_keeps_one_session_and_closes_on_disconnect() {
        i_slint_backend_testing::init_no_event_loop();
        let data = tempfile::tempdir().unwrap();
        let window = YapfastWindow::new().unwrap();
        let adapter = Adapter::new(
            Arc::new(BackgroundRuntime::new().unwrap()),
            data.path().into(),
            &window,
        )
        .unwrap();
        bind_callbacks(&window, &adapter);
        {
            let mut a = adapter.borrow_mut();
            a.app.state.voice.room = Some("discord:@me:10".into());
            a.app.state.voice.phase = Phase::Connected;
            a.refresh(&window);
        }
        window.invoke_pop_out_call();
        assert!(window.get_call_popped());
        let first = adapter
            .borrow()
            .call_window
            .as_ref()
            .unwrap()
            .view
            .as_weak();
        window.invoke_pop_out_call();
        assert!(first.upgrade().is_some());
        {
            let mut a = adapter.borrow_mut();
            a.close_call_window(&window);
            assert!(a.app.state.voice.active());
        }
        assert!(!window.get_call_popped());
        window.invoke_pop_out_call();
        let mut a = adapter.borrow_mut();
        a.app.state.voice.phase = Phase::Disconnected;
        a.refresh(&window);
        assert!(!window.get_call_popped());
        assert!(a.call_window.is_none());
    }
}
