//! Scheduled sends share the already-authorized Discord::send upload/message
//! service with the normal composer. No credentials or second client here.
use crate::scheduled::{Clock, Dispatch};
use yapfast_discord::service::Snapshot;

fn permit(send: &Dispatch, snapshot: &Snapshot, clock: &impl Clock) -> Result<(), String> {
    use yapfast_discord::service::Status;
    if send.claim.is_cancelled() {
        return Err(
            "Scheduled claim cancelled by account/storage lifecycle. No automatic retry.".into(),
        );
    }
    if snapshot.status != Status::Online
        || snapshot.user_id.as_deref() != Some(send.account.as_str())
    {
        return Err("The scheduled message's owning account is not connected.".into());
    }
    if clock.now_ms() < send.due_ms {
        return Err(
            "Wall clock moved back before the scheduled due time; review and reschedule.".into(),
        );
    }
    if send.id.len() != 24 || !send.id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid scheduled message ID.".into());
    }
    crate::scheduled::validate_contents(&send.account, &send.room, &send.body, send.files.len())?;
    if clock.now_ms().saturating_sub(send.due_ms) > crate::scheduled::MAX_LATENESS_MS {
        return Err("This message is more than 24 hours late. Review and reschedule it.".into());
    }
    let (rooms, _) = crate::discord::index(snapshot, false);
    let room = rooms
        .iter()
        .find(|r| r.id == send.room)
        .filter(|r| r.has_text() && !r.forum && !r.locked && !r.read_only)
        .ok_or("The scheduled channel is unavailable or this account cannot send there.")?;
    if send.files.iter().any(|f| f.bytes > room.upload_limit) {
        return Err(
            "A scheduled attachment exceeds this channel/account's current upload limit.".into(),
        );
    }
    Ok(())
}

pub(super) fn send(
    payload: Dispatch,
    discord: Result<&yapfast_discord::service::Discord, &str>,
    channels: &mut super::Channels,
    events: &tokio::sync::mpsc::UnboundedSender<crate::chat::Event>,
) {
    let fail = |error: String| {
        let _ = events.send(crate::chat::Event::ScheduledResult {
            id: payload.id.clone(),
            account: payload.account.clone(),
            result: Err(error),
        });
    };
    let discord = match discord {
        Ok(discord) => discord,
        Err(error) => {
            fail(error.into());
            return;
        }
    };
    if let Err(error) = permit(
        &payload,
        &discord.snapshots().borrow(),
        &crate::scheduled::SystemClock,
    ) {
        fail(error);
        return;
    }
    let key = (payload.account.clone(), payload.id.clone());
    if channels.scheduled_attempts.len() >= 4096 || !channels.scheduled_attempts.insert(key) {
        fail("Duplicate scheduled claim, or the session's safety limit was reached. Check the channel; no retry.".into());
        return;
    }
    let (discord, events) = (discord.clone(), events.clone());
    tokio::spawn(async move {
        let result = payload
            .claim
            .run(async {
                let files = payload.files.clone();
                let files =
                    tokio::task::spawn_blocking(move || crate::scheduled::read_attachments(&files))
                        .await
                        .map_err(|_| "Could not read scheduled attachments.".to_owned())??;
                permit(
                    &payload,
                    &discord.snapshots().borrow(),
                    &crate::scheduled::SystemClock,
                )?;
                let (guild, channel) = crate::discord::parse_room(&payload.room)
                    .ok_or("Invalid scheduled channel.")?;
                let silent_text = payload
                    .body
                    .strip_prefix("@silent")
                    .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace));
                let options = yapfast_discord::api::Sending {
                    nonce: &payload.id,
                    enforce_nonce: true,
                    reply: None,
                    ping_reply: false,
                    silent: silent_text.is_some(),
                };
                // Exact same authorized upload + send path as OpenChannel::start_send.
                // One request attempt; a timeout is ambiguous, never auto-retried.
                let message = tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    discord.send(
                        guild,
                        channel,
                        silent_text.map_or(payload.body.as_str(), str::trim_start),
                        options,
                        files,
                    ),
                )
                .await
                .map_err(|_| "Scheduled send timed out; delivery is uncertain.".to_owned())?
                .map_err(|e| e.to_string())?;
                if message.channel_id != channel
                    || message.author_id != payload.account
                    || message
                        .nonce
                        .as_ref()
                        .is_some_and(|nonce| nonce != &payload.id)
                {
                    return Err(
                        "Discord's message receipt did not match the scheduled send.".into(),
                    );
                }
                Ok(())
            })
            .await
            .and_then(|result| result);
        let _ = events.send(crate::chat::Event::ScheduledResult {
            id: payload.id,
            account: payload.account,
            result,
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Time(i64);
    impl Clock for Time {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }
    fn payload() -> Dispatch {
        Dispatch {
            id: "0123456789abcdef00000001".into(),
            account: "123".into(),
            room: "discord:10:11".into(),
            body: "hello".into(),
            due_ms: 1000,
            files: vec![],
            claim: Default::default(),
        }
    }
    #[test]
    fn revoked_backend_claim_is_rejected_before_account_or_provider_access() {
        let send = payload();
        send.claim.revoke();
        let error = permit(&send, &Snapshot::default(), &Time(1000)).unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
    }
    #[test]
    fn scheduled_backend_rechecks_account_wall_time_connection_and_channel_permission() {
        use yapfast_discord::{marked::MarkedServer, ready, service::Status};
        let mut snapshot = Snapshot {
            user_id: Some("123".into()),
            status: Status::Online,
            servers: vec![MarkedServer::from(
                ready::server(&serde_json::json!({
                    "id":"10", "owner_id":"123", "properties":{"name":"Fixture"},
                    "roles":[{"id":"10","name":"@everyone","position":0,"permissions":"0"}],
                    "channels":[{"id":"11","type":0,"name":"general"}]
                }))
                .unwrap(),
            )],
            ..Default::default()
        };
        let send = payload();
        assert!(
            permit(&send, &snapshot, &Time(999)).is_err(),
            "never before due"
        );
        assert!(permit(&send, &snapshot, &Time(1000)).is_ok());
        snapshot.user_id = Some("456".into());
        assert!(permit(&send, &snapshot, &Time(1000)).is_err());
        snapshot.user_id = Some("123".into());
        snapshot.servers.clear();
        assert!(permit(&send, &snapshot, &Time(1000)).is_err());
        snapshot.status = Status::Connecting;
        assert!(permit(&send, &snapshot, &Time(1000)).is_err());
    }
}
