//! Explicit received-only Discord read review. Matrix is disabled, not repaired.
use super::*;
use std::time::{Duration, Instant};
pub const READ_ALL_LIMIT: usize = 100;
#[derive(Clone, Debug)]
pub struct ReadAll {
    pub rooms: Vec<String>,
    pub eligible: usize,
    pub sync: bool,
    owner: Option<String>,
    #[cfg(feature = "discord")]
    source: Option<yapfast_discord::service::PresenceObservation>,
}
fn eligible(state: &AppState, room: &ChatRoom) -> bool {
    room.network == Network::Discord
        && state.discord_online
        && state.discord_user.is_some()
        && !state.preview
        && !room.locked
        && !room.request
        && room.has_text()
        && (room.unread > 0 || room.mentions > 0)
}
impl Application {
    pub(super) fn clear_read_all(&mut self) {
        self.state.read_all = None;
        self.read_all_submitted.clear();
    }
    pub(super) fn ask_read_all(&mut self) -> Vec<Effect> {
        if self.state.preview {
            return vec![];
        }
        let now = Instant::now();
        self.read_all_submitted.retain(|id, (marker, sent)| {
            now.duration_since(*sent) < Duration::from_secs(30)
                && self
                    .state
                    .rooms
                    .iter()
                    .any(|r| &r.id == id && eligible(&self.state, r) && &r.last_read == marker)
        });
        let mut rooms: Vec<_> = self
            .state
            .rooms
            .iter()
            .filter(|r| eligible(&self.state, r) && !self.read_all_submitted.contains_key(&r.id))
            .map(|r| r.id.clone())
            .collect();
        rooms.sort();
        rooms.dedup();
        let count = rooms.len();
        rooms.truncate(READ_ALL_LIMIT);
        self.state.read_all = Some(ReadAll {
            rooms,
            eligible: count,
            sync: self.state.sync_discord_reads,
            owner: self.state.discord_user.clone(),
            #[cfg(feature = "discord")]
            source: self
                .discord
                .as_ref()
                .and_then(|s| s.presence_observation.clone()),
        });
        vec![]
    }
    pub(super) fn confirm_read_all(&mut self) -> Vec<Effect> {
        let Some(plan) = self.state.read_all.take() else {
            return vec![];
        };
        if self.state.preview || !self.state.discord_online || plan.owner != self.state.discord_user
        {
            return vec![];
        }
        #[cfg(feature = "discord")]
        if match (
            &plan.source,
            self.discord
                .as_ref()
                .and_then(|s| s.presence_observation.as_ref()),
        ) {
            (Some(a), Some(b)) => !a.same_source(b),
            (None, None) => false,
            _ => true,
        } {
            return vec![];
        }
        let now = Instant::now();
        let effects: Vec<_> = plan
            .rooms
            .into_iter()
            .filter_map(|id| {
                let room = self
                    .state
                    .rooms
                    .iter()
                    .find(|r| r.id == id && eligible(&self.state, r))?;
                if self.read_all_submitted.contains_key(&id) {
                    return None;
                }
                self.read_all_submitted
                    .insert(id.clone(), (room.last_read.clone(), now));
                Some(Effect::Backend(Command::MarkRead {
                    room: id,
                    sync: plan.sync,
                }))
            })
            .collect();
        self.state.notice = Some(format!(
            "Requested read state for {} received Discord conversations. Awaiting received state; unacknowledged requests can be reviewed again after 30 seconds. Matrix is not included.",
            effects.len()
        ));
        effects
    }
}
