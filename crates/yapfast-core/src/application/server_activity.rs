//! Aggregates only received state. No joins, probes, polling or server totals.
use super::*;

impl AppState {
    /// Distinct participants and Go Live streams in this server's visible voice
    /// channels. Offline/unavailable/unknown context cannot display cached activity.
    pub fn received_server_activity(&self, server: &ChatServer) -> (usize, usize) {
        if !self.discord_online
            || self.discord_user.is_none()
            || !server.discord
            || server.unavailable
        {
            return (0, 0);
        }
        let channels: HashSet<_> = server.channels.iter().map(String::as_str).collect();
        let mut people = HashMap::new();
        for room in self.rooms.iter().filter(|room| {
            room.network == Network::Discord
                && room.voice
                && !room.direct
                && !room.locked
                && channels.contains(room.id.as_str())
        }) {
            for person in &room.people {
                people
                    .entry(&person.id)
                    .and_modify(|live| *live |= person.streaming)
                    .or_insert(person.streaming);
            }
        }
        (people.len(), people.values().filter(|&&live| live).count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn received_activity_deduplicates_and_rejects_hidden_other_network_and_stale_state() {
        let mut state = AppState::default();
        state.discord_online = true;
        state.discord_user = Some("100".into());
        let mut server = ChatServer {
            discord: true,
            channels: vec![
                "one".into(),
                "two".into(),
                "locked".into(),
                "dm".into(),
                "matrix".into(),
            ],
            ..Default::default()
        };
        let room = |id: &str, streaming| {
            Arc::new(ChatRoom {
                network: Network::Discord,
                voice: true,
                people: vec![chat::VoicePerson {
                    id: "200".into(),
                    streaming,
                    ..Default::default()
                }],
                ..preview::preview_room(id, id, false, 10)
            })
        };
        state.rooms = vec![
            room("one", false),
            room("two", true),
            Arc::new(ChatRoom {
                locked: true,
                ..(*room("locked", true)).clone()
            }),
            Arc::new(ChatRoom {
                direct: true,
                ..(*room("dm", true)).clone()
            }),
            Arc::new(ChatRoom {
                network: Network::Matrix,
                ..(*room("matrix", true)).clone()
            }),
            room("other-server", true),
        ];
        assert_eq!(state.received_server_activity(&server), (1, 1));
        state.rooms.retain(|room| room.id != "two");
        assert_eq!(state.received_server_activity(&server), (1, 0));
        state.discord_online = false;
        assert_eq!(state.received_server_activity(&server), (0, 0));
        state.discord_online = true;
        server.unavailable = true;
        assert_eq!(state.received_server_activity(&server), (0, 0));
        server.unavailable = false;
        state.discord_user = None;
        assert_eq!(state.received_server_activity(&server), (0, 0));
        state.discord_user = Some("101".into());
        state.rooms.clear();
        assert_eq!(state.received_server_activity(&server), (0, 0));
    }
}
