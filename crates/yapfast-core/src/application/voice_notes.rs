//! Voice-note sends are independent of text drafts, replies and file staging.
use super::*;
use yapfast_discord::voice_note::Note;

#[derive(Clone, Debug)]
pub struct VoiceNoteDelivery {
    pub account: String,
    pub request: String,
    pub result: Option<Result<(), String>>,
}

impl Application {
    /// A live capability check, repeated before opening a microphone and Send.
    pub fn voice_note_problem(&self, room: &str) -> Option<String> {
        if self.state.preview || !self.state.discord_online || self.state.discord_user.is_none() {
            return Some("Connect to Discord to record a voice message.".into());
        }
        if self.state.selected.as_deref() != Some(room)
            || !self.state.can_write()
            || self.state.edit().is_some()
        {
            return Some(
                "Open a writable conversation before recording or sending a voice message.".into(),
            );
        }
        let Some(selected) = self.state.selected_room() else {
            return Some("Open a conversation to record a voice message.".into());
        };
        if selected.network != Network::Discord {
            return Some("Open a Discord conversation to record a voice message.".into());
        }
        if selected.request {
            return Some("Accept the message request before sending a recording.".into());
        }
        if !selected.direct {
            let Some((server, Some(channel), me)) = self.discord_place(room) else {
                return Some(
                    "Voice-message permissions are not known for this conversation.".into(),
                );
            };
            let Some(channel) = server.server.channel(channel) else {
                return Some("Voice-message channel is unavailable.".into());
            };
            let required = yapfast_discord::voice_note::REQUIRED_PERMISSIONS;
            if server.channel_permissions(channel, me, &server.marks.roles) & required != required {
                return Some(
                    "This channel requires Attach Files and Send Voice Messages permissions."
                        .into(),
                );
            }
        }
        if self.state.voice_note_delivery.as_ref().is_some_and(|sent| {
            Some(&sent.account) == self.state.discord_user.as_ref() && sent.result.is_none()
        }) {
            return Some("A voice message is already uploading. Wait for its result.".into());
        }
        None
    }

    pub(super) fn send_voice_note(
        &mut self,
        room: String,
        account: String,
        request: String,
        note: Arc<Note>,
    ) -> Vec<Effect> {
        let problem = self
            .voice_note_problem(&room)
            .or_else(|| {
                (self.state.discord_user.as_deref() != Some(&account))
                    .then(|| "Reopen the recorder for your current account.".to_owned())
            })
            .or_else(|| note.validate().err().map(|error| error.to_string()))
            .or_else(|| {
                self.state
                    .selected_room()
                    .filter(|room| note.data().len() as u64 > room.upload_limit)
                    .map(|_| "The voice note exceeds this conversation's upload limit.".to_owned())
            });
        if let Some(problem) = problem {
            self.state.notice = Some(problem);
            return vec![];
        }
        self.state.voice_note_delivery = Some(VoiceNoteDelivery {
            account: account.clone(),
            request: request.clone(),
            result: None,
        });
        vec![Effect::Backend(Command::SendVoiceNote {
            room,
            account,
            request,
            note,
        })]
    }
}
