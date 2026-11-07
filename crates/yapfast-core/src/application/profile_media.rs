//! The one native viewer can show a received profile asset without creating
//! fake messages, attachment indexes, or a general privileged URL fetch API.
use super::*;
use crate::profile_media::{Session, Slot};

#[cfg(test)]
#[path = "profile_media_tests.rs"]
mod tests;

/// This guard belongs to Application, never to clonable Viewer/Session state.
pub(super) struct ProfileOriginal {
    request: u64,
    pub(super) cancellation: crate::original::Request,
}

impl Drop for ProfileOriginal {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl Application {
    pub(super) fn forget_profile_media_authority(&mut self) {
        // Never rebind a previous account's received profile to a new account.
        // Matrix profiles use a different provider and are left alone.
        for view in [&mut self.state.profile, &mut self.state.panel_profile] {
            if view
                .as_ref()
                .is_some_and(|view| !view.profile.id.starts_with('@'))
            {
                *view = None;
            }
        }
    }
    pub(super) fn profile_media_request(&self) -> Option<u64> {
        self.profile_original.as_ref().map(|active| active.request)
    }

    fn media_profile(&self, slot: Slot) -> Option<&Profile> {
        let view = match slot {
            Slot::Profile => self.state.profile.as_ref(),
            Slot::Panel => self.state.panel_profile.as_ref(),
        }?;
        // Don't let an interim id/name shell become a resource authority.
        (!view.loading || view.kept).then_some(&view.profile)
    }

    pub(super) fn view_profile_media(
        &mut self,
        user: String,
        slot: Slot,
        kind: chat::ProfilePicture,
    ) -> Vec<Effect> {
        if self.state.preview || !self.state.discord_online {
            return vec![];
        }
        let Some(profile) = self
            .media_profile(slot)
            .filter(|profile| profile.id == user)
        else {
            return vec![];
        };
        let Some(session) = self
            .state
            .discord_user
            .as_deref()
            .and_then(|account| Session::open(account, profile, slot, kind))
        else {
            return vec![];
        };
        let cancellation = crate::original::Request::new();
        let command = DiscordCommand::ViewProfileMedia {
            request: session.request,
            asset: session.asset.clone(),
            cancellation: cancellation.clone(),
        };
        self.profile_original = Some(ProfileOriginal {
            request: session.request,
            cancellation,
        });
        self.state.viewer = Some(Viewer {
            source: ViewerSource::Profile(session),
            ..Default::default()
        });
        vec![Effect::Backend(Command::Discord(command))]
    }

    pub(super) fn reconcile_profile_media(&mut self, before: Option<u64>) -> Option<Effect> {
        if let Some(Viewer {
            source: ViewerSource::Profile(session),
            ..
        }) = &self.state.viewer
            && !session.is_current(
                self.state.discord_user.as_deref(),
                self.media_profile(session.slot),
            )
        {
            self.state.viewer = None;
        }
        let selected = self
            .state
            .viewer
            .as_ref()
            .and_then(|viewer| match &viewer.source {
                ViewerSource::Profile(session) => Some(session.request),
                ViewerSource::Message => None,
            });
        if self.profile_media_request() != selected {
            // Dropping the guard cancels immediately, even without delivery
            // of the cleanup command, and never clears the replacement viewer.
            self.profile_original = None;
        }
        if let Some(active) = &self.profile_original
            && active.cancellation.is_cancelled()
            && let Some(Viewer {
                source: ViewerSource::Profile(session),
                ..
            }) = &mut self.state.viewer
            && session.request == active.request
            && session.loaded.is_none()
        {
            // A provider may drop its loader before a completion can be sent.
            // Keep an actionable error, not an immortal Loading presentation.
            session.loaded = Some(crate::profile_media::Loaded {
                bytes: None,
                picture: Err("Profile media stopped loading. Reopen the image to try again."),
            });
        }
        before
            .filter(|before| Some(*before) != self.profile_media_request())
            .map(|request| {
                Effect::Backend(Command::Discord(DiscordCommand::CancelProfileMedia(
                    request,
                )))
            })
    }

    pub(super) fn profile_media_loaded(
        &mut self,
        request: u64,
        asset: &crate::profile_media::Asset,
        loaded: crate::original::Completion<crate::profile_media::Loaded>,
    ) {
        if !self
            .profile_original
            .as_ref()
            .is_some_and(|active| active.request == request && !active.cancellation.is_cancelled())
        {
            return;
        }
        let current = self
            .state
            .viewer
            .as_ref()
            .is_some_and(|viewer| match &viewer.source {
                ViewerSource::Profile(session) => {
                    session.request == request
                        && session.asset == *asset
                        && session.is_current(
                            self.state.discord_user.as_deref(),
                            self.media_profile(session.slot),
                        )
                }
                ViewerSource::Message => false,
            });
        if current
            && let Some(Viewer {
                source: ViewerSource::Profile(session),
                ..
            }) = &mut self.state.viewer
        {
            session.accept(request, asset, loaded.into_inner());
        }
        // A stale envelope drops buffers and its lease together; it is never
        // turned into an accepted/clonable presentation snapshot.
    }

    pub(super) fn save_profile_media(&mut self, request: u64, path: PathBuf) -> Vec<Effect> {
        let Some(Viewer {
            source: ViewerSource::Profile(session),
            ..
        }) = &self.state.viewer
        else {
            return vec![];
        };
        if session.request != request
            || !session.is_current(
                self.state.discord_user.as_deref(),
                self.media_profile(session.slot),
            )
        {
            return vec![];
        }
        let Some(bytes) = session
            .loaded
            .as_ref()
            .and_then(|loaded| loaded.bytes.clone())
        else {
            return vec![];
        };
        let account = session.asset.account().to_owned();
        if let Some(Viewer {
            source: ViewerSource::Profile(session),
            ..
        }) = &mut self.state.viewer
        {
            session.save_note = Some("Saving image...");
        }
        vec![Effect::Backend(Command::Discord(
            DiscordCommand::SaveProfileMedia {
                request,
                account,
                bytes,
                path,
            },
        ))]
    }
}
