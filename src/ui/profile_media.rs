//! Profile assets enter the existing ImageViewer. No web/native second viewer.
use super::*;
use yapfast_core::{application::ViewerSource, profile_media::Slot};

impl Adapter {
    pub(super) fn show_profile_media(&mut self, window: &YapfastWindow) -> bool {
        let Some(viewer) = &self.app.state.viewer else {
            return false;
        };
        let ViewerSource::Profile(session) = &viewer.source else {
            return false;
        };
        window.set_viewer_open(true);
        window.set_viewer_profile_media(true);
        window.set_viewer_source_key(session.request.to_string().into());
        window.set_viewer_media_link(session.asset.url().into());
        window.set_viewer_name(session.asset.name().into());
        // These remain strictly message-only. Profile actions have their own
        // callbacks and a request id captured before the async save dialog.
        window.set_viewer_item(SharedString::default());
        window.set_viewer_index(0);
        window.set_viewer_position(SharedString::default());
        window.set_viewer_save_ready(
            session
                .loaded
                .as_ref()
                .is_some_and(|loaded| loaded.bytes.is_some()),
        );
        window.set_viewer_note(
            session
                .save_note
                .unwrap_or_else(
                    || match session.loaded.as_ref().map(|loaded| &loaded.picture) {
                        None => "Loading the maximum available profile image...",
                        Some(Err(error)) => error,
                        Some(Ok(_)) => {
                            "Maximum CDN size requested; the upload may contain less detail."
                        }
                    },
                )
                .into(),
        );
        let profile = match session.slot {
            Slot::Profile => self.app.state.profile.as_ref(),
            Slot::Panel => self.app.state.panel_profile.as_ref(),
        };
        let preview = profile
            .and_then(|view| match session.asset.kind() {
                ProfilePicture::Avatar => view.profile.avatar.as_ref(),
                ProfilePicture::Banner => view.profile.banner.as_ref(),
            })
            .and_then(|url| self.app.state.pictures.get(url));
        let shown = session
            .loaded
            .as_ref()
            .and_then(|loaded| loaded.picture.as_ref().ok())
            .or(preview);
        let image = match (shown, &self.viewer_image) {
            (Some(shown), Some((cached, image))) if Arc::ptr_eq(shown, cached) => image.clone(),
            (Some(shown), _) => {
                let image = picture(shown);
                self.viewer_image = Some((shown.clone(), image.clone()));
                image
            }
            (None, _) => {
                self.viewer_image = None;
                slint::Image::default()
            }
        };
        let (width, height) = shown
            .map(|picture| (picture.width, picture.height))
            .unwrap_or((1, 1));
        window.set_viewer_width(width as i32);
        window.set_viewer_height(height as i32);
        window.set_viewer_image(image);
        let (frames, delays) = shown.map(animation).unwrap_or_default();
        window.set_viewer_frames(frames);
        window.set_viewer_delays(delays);
        self.viewer_size = None;
        true
    }
}

pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    let state = adapter.clone();
    let weak = window.as_weak();
    window.on_view_profile_media(move |user, panel, kind| {
        let kind = match kind {
            0 => ProfilePicture::Avatar,
            1 => ProfilePicture::Banner,
            _ => return,
        };
        let Some(window) = weak.upgrade() else {
            return;
        };
        state.borrow_mut().action(
            &window,
            Action::ViewProfileMedia {
                user: user.into(),
                slot: if panel { Slot::Panel } else { Slot::Profile },
                kind,
            },
        );
    });
    let state = adapter.clone();
    window.on_save_profile_media(move || {
        let target = {
            let adapter = state.borrow();
            adapter
                .app
                .state
                .viewer
                .as_ref()
                .and_then(|viewer| match &viewer.source {
                    ViewerSource::Profile(session)
                        if session
                            .loaded
                            .as_ref()
                            .is_some_and(|loaded| loaded.bytes.is_some()) =>
                    {
                        Some((session.request, session.asset.name().to_owned()))
                    }
                    _ => None,
                })
        };
        let Some((request, name)) = target else {
            return;
        };
        let weak = Rc::downgrade(&state);
        state.borrow_mut().dialog(async move {
            let file = rfd::AsyncFileDialog::new()
                .set_title("Save profile image")
                .set_file_name(name)
                .save_file()
                .await;
            if let (Some(file), Some(adapter)) = (file, weak.upgrade()) {
                let window = adapter.borrow().window.upgrade();
                if let Some(window) = window {
                    adapter.borrow_mut().action(
                        &window,
                        Action::SaveProfileMedia {
                            request,
                            path: file.path().to_owned(),
                        },
                    );
                }
            }
        });
    });
}
