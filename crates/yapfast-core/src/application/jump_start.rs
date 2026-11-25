//! One bounded Discord boundary request. Matrix history remains explicitly held.
use super::*;
impl Application {
    /// Whether the current room still owns an in-flight history-start seek.
    /// An empty action result alone cannot distinguish deduplication from refusal.
    pub fn is_seeking_start(&self) -> bool {
        self.seeking.as_ref().is_some_and(|seek| seek.item == "d:1")
    }

    pub(super) fn jump_to_start(&mut self) -> Vec<Effect> {
        let Some(room) = self
            .state
            .selected_room()
            .filter(|room| !room.locked && self.state.shows_text())
        else {
            return vec![];
        };
        if room.network != Network::Discord {
            self.state.notice = Some(
                "Jump to history start is currently supported on Discord only. Matrix is disabled."
                    .into(),
            );
            return vec![];
        }
        if self.state.preview
            || self.state.busy
            || !self.state.room_ready
            || !self.state.discord_online
            || self.seeking.as_ref().is_some_and(|s| s.item == "d:1")
        {
            return vec![];
        }
        self.seeking = Some(Seeking {
            item: "d:1".into(),
            unread: false,
        });
        self.focus("d:1".into())
    }
}
