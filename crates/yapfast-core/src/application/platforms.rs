//! Received device evidence has the provider/account lifetime, not cached profile lifetime.
use super::*;

#[cfg(feature = "discord")]
pub(super) fn accept_observation(
    kept: &mut HashMap<String, yapfast_discord::service::PresenceObservation>,
    id: &str,
    observation: Option<&yapfast_discord::service::PresenceObservation>,
) -> bool {
    match observation {
        Some(observation) => {
            if kept
                .get(id)
                .is_some_and(|earlier| !observation.newer_than(earlier))
            {
                return false;
            }
            kept.insert(id.to_owned(), observation.clone());
            true
        }
        // Unproven local data cannot overwrite received source-ordered evidence.
        None => !kept.contains_key(id),
    }
}

fn unknown_platform(presence: Option<chat::Presence>) -> Option<chat::Presence> {
    presence.map(|presence| match presence {
        #[cfg(feature = "discord")]
        chat::Presence::Clients(clients) => match clients.base() {
            yapfast_discord::member_list::Presence::Mobile => chat::Presence::Online,
            other => discord::presence(other),
        },
        chat::Presence::Mobile => chat::Presence::Online,
        other => other,
    })
}
impl Application {
    /// Initialize a newly opened card from exact received evidence. Member
    /// receipt and coalesced snapshot publication are independent.
    pub(super) fn profile_platform_presence(
        &mut self,
        id: &str,
        fallback: Option<chat::Presence>,
    ) -> Option<chat::Presence> {
        if let Some(presence) = self.platform_presences.get(id) {
            return *presence;
        }
        #[cfg(feature = "discord")]
        if let Some(snapshot) = &self.discord {
            let observation = snapshot
                .friend_list
                .iter()
                .find(|friend| friend.user_id == id)
                .and_then(|friend| friend.presence_observation.clone())
                .or_else(|| {
                    snapshot
                        .conversations
                        .iter()
                        .find(|dm| !dm.group && dm.person.as_deref() == Some(id))
                        .and_then(|dm| dm.presence_observation.clone())
                });
            if let Some(observation) = observation {
                self.platform_observations
                    .insert(id.to_owned(), observation);
            }
        }
        self.platform_presences.insert(id.to_owned(), fallback);
        fallback
    }

    #[cfg(feature = "discord")]
    pub(super) fn received_member_platforms(
        &mut self,
        room: &str,
        members: &mut [ChatMember],
        observations: HashMap<String, yapfast_discord::service::PresenceObservation>,
        observation: Option<yapfast_discord::service::PresenceObservation>,
    ) -> bool {
        if self.state.selected.as_deref() != Some(room) {
            return false;
        }
        let source = self
            .discord
            .as_ref()
            .filter(|snapshot| snapshot.status == yapfast_discord::service::Status::Online)
            .and_then(|snapshot| snapshot.presence_observation.clone());
        // Cached membership/names and its legacy base dot remain useful offline;
        // a deserialized row carries no live platform authority.
        for member in members.iter_mut() {
            if !source.as_ref().is_some_and(|source| {
                observations
                    .get(&member.id)
                    .is_some_and(|receipt| receipt.same_source(source))
            }) {
                member.presence = unknown_platform(member.presence);
            }
        }
        let Some(source) = source else {
            return true;
        };
        // Queued events from a retired provider/session have no authority.
        if observation
            .as_ref()
            .is_some_and(|receipt| !receipt.same_source(&source))
        {
            return false;
        }
        let ids: HashSet<_> = self
            .platform_presences
            .keys()
            .cloned()
            .chain(members.iter().map(|member| member.id.clone()))
            .collect();
        for id in ids {
            let member = members.iter().find(|member| member.id == id);
            let previously_listed = self
                .state
                .members
                .as_ref()
                .and_then(|list| list.as_ref().ok())
                .is_some_and(|list| list.iter().any(|member| member.id == id));
            if member.is_none() && !previously_listed {
                continue;
            }
            let receipt = if member.is_some() {
                observations.get(&id)
            } else {
                observation.as_ref()
            };
            // A cached/unproven member row must not become fresh on publication.
            let Some(receipt) = receipt.filter(|receipt| receipt.same_source(&source)) else {
                continue;
            };
            if !accept_observation(&mut self.platform_observations, &id, Some(receipt)) {
                continue;
            }
            let presence = member.and_then(|member| member.presence);
            self.platform_presences.insert(id.clone(), presence);
            for view in [&mut self.state.profile, &mut self.state.panel_profile]
                .into_iter()
                .flatten()
                .filter(|view| view.profile.id == id)
            {
                view.platform_presence = presence;
            }
        }
        true
    }

    pub(super) fn clear_received_platforms(&mut self) {
        #[cfg(feature = "discord")]
        self.platform_observations.clear();
        for presence in self.platform_presences.values_mut() {
            *presence = None;
        }
        for friend in &mut self.state.friends {
            friend.presence = unknown_platform(friend.presence);
        }
        for room in &mut self.state.rooms {
            let room = Arc::make_mut(room);
            room.presence = unknown_platform(room.presence);
        }
        if let Some(Ok(members)) = &mut self.state.members {
            for member in members {
                member.presence = unknown_platform(member.presence);
            }
        }
        for view in [&mut self.state.profile, &mut self.state.panel_profile]
            .into_iter()
            .flatten()
        {
            view.profile.presence = unknown_platform(view.profile.presence);
            view.platform_presence = None;
        }
    }
}
