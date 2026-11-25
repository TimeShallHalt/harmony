use super::{Command, Page, Sender, Shell};
use crate::{MobilePoll, MobileWindow, PollChoice, clock};
use anyhow::{Context, Result, ensure};
use slint::{ComponentHandle, Model, VecModel};
use std::rc::Rc;
use yapfast_discord::{
    api::{NewAnswer, NewPoll},
    message::{self, Message, Poll, PollAnswer},
    ready::{PRIVATE, permission::SEND_POLLS},
    service::Live,
};

const HOURS: [u32; 7] = [1, 4, 8, 24, 72, 168, 336];
pub(crate) enum Event {
    Create {
        room: String,
        poll: NewPoll,
    },
    Vote {
        room: String,
        item: String,
        answers: Vec<u64>,
    },
    End {
        room: String,
        item: String,
    },
}

impl Event {
    fn room(&self) -> &str {
        match self {
            Self::Create { room, .. } | Self::Vote { room, .. } | Self::End { room, .. } => room,
        }
    }
    fn mode(&self) -> &'static str {
        match self {
            Self::Create { .. } => "poll-create",
            Self::Vote { .. } => "poll-vote",
            Self::End { .. } => "poll-end",
        }
    }
}

fn submit(weak: &slint::Weak<MobileWindow>, tx: &Sender, build: impl FnOnce(String) -> Event) {
    let Some(w) = weak.upgrade() else {
        return;
    };
    if w.get_poll_busy() {
        return;
    }
    w.set_poll_busy(true);
    w.set_poll_error("".into());
    if tx
        .send(Command::Poll(build(w.get_selected_room().to_string())))
        .is_err()
    {
        w.set_poll_busy(false);
        w.set_poll_error("The connection has closed.".into());
    }
}

pub(super) fn bind(w: &MobileWindow, tx: Sender) {
    let (weak, create_tx) = (w.as_weak(), tx.clone());
    w.on_post_poll(move |question, answers, hours, multi| {
        let poll = NewPoll {
            question: question.trim().to_owned(),
            answers: answers
                .iter()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .map(|text| NewAnswer { text, emoji: None })
                .collect(),
            hours: hours.max(0) as u32,
            multiselect: multi,
        };
        submit(&weak, &create_tx, move |room| Event::Create { room, poll });
    });
    let (weak, vote_tx) = (w.as_weak(), tx.clone());
    w.on_vote_poll(move |item, answer_ids| {
        let answers = answer_ids
            .iter()
            .filter(|id| *id > 0)
            .map(|id| id as u64)
            .collect();
        submit(&weak, &vote_tx, move |room| Event::Vote {
            room,
            item: item.into(),
            answers,
        });
    });
    let weak = w.as_weak();
    w.on_end_poll(move |item| {
        submit(&weak, &tx, move |room| Event::End {
            room,
            item: item.into(),
        });
    });
}

fn closed(poll: &Poll) -> bool {
    poll.finalized || poll.expiry.is_some_and(|at| at <= clock::now_ms())
}

pub(crate) fn card(message: &Message, me: &str, can_vote: bool) -> MobilePoll {
    let Some(poll) = &message.poll else {
        return MobilePoll::default();
    };
    let closed = closed(poll);
    let total = poll.answers.iter().map(|a| a.votes).sum::<usize>();
    let ends = poll
        .expiry
        .map(|at| format!("{} at {}", clock::day(at / 1000), clock::time(at / 1000)))
        .unwrap_or_default();
    let choices = poll
        .answers
        .iter()
        .map(|a| {
            let label = match &a.emoji {
                Some((symbol, _)) => format!("{symbol} {}", a.text),
                None => a.text.clone(),
            };
            PollChoice {
                id: a.id.try_into().unwrap_or(i32::MAX),
                label: label.into(),
                mine: a.me,
                votes: a.votes.try_into().unwrap_or(i32::MAX),
                percent: if total == 0 {
                    0
                } else {
                    ((a.votes as f64 / total as f64) * 100.0).round() as i32
                },
            }
        })
        .collect::<Vec<_>>();
    MobilePoll {
        question: poll.question.clone().into(),
        choices: Rc::new(VecModel::from(choices)).into(),
        multiple: poll.multiselect,
        counted: poll.counted,
        closed,
        expires_in: poll.expiry.map_or(0, |at| {
            at.saturating_sub(clock::now_ms())
                .div_ceil(1000)
                .clamp(1, i32::MAX as u64) as i32
        }),
        ends: ends.into(),
        total: total.try_into().unwrap_or(i32::MAX),
        voted: poll.answers.iter().any(|a| a.me),
        can_vote: can_vote && !closed,
        can_end: !closed && !me.is_empty() && message.author_id == me,
    }
}

fn validate(poll: &NewPoll) -> Result<()> {
    ensure!(
        !poll.question.is_empty() && poll.question.chars().count() <= 300,
        "Enter a question of 1–300 characters."
    );
    ensure!(
        (1..=10).contains(&poll.answers.len()),
        "Enter between one and ten answers."
    );
    ensure!(
        poll.answers
            .iter()
            .all(|a| !a.text.is_empty() && a.text.chars().count() <= 55),
        "Each answer can contain up to 55 characters."
    );
    ensure!(
        HOURS.contains(&poll.hours),
        "Choose a listed poll duration."
    );
    Ok(())
}

impl Shell {
    pub(super) fn can_create_poll(&self) -> bool {
        if self.page != Page::Chat || self.room.request || !self.room.writable || !self.online() {
            return false;
        }
        if self.preview || self.room.guild == PRIVATE {
            return true;
        }
        self.snapshot
            .servers
            .iter()
            .find(|g| g.id == self.room.guild)
            .is_some_and(|s| {
                s.channel(&self.room.id).is_some_and(|c| {
                    s.channel_permissions(c.channel, self.me(), &s.marks.roles) & SEND_POLLS != 0
                })
            })
    }

    pub(super) fn apply_poll_vote(&mut self, event: &Live) -> bool {
        let Live::Vote {
            channel,
            message,
            answer,
            me,
            added,
        } = event
        else {
            return false;
        };
        if channel != &self.room.id {
            return false;
        }
        if let Some(post) = self.posts.iter_mut().find(|p| p.id == *message) {
            post.vote(*answer, *me, *added);
            return true;
        }
        false
    }

    pub(super) async fn poll_event(&mut self, event: Event) {
        let room = event.room().to_owned();
        let mode = event.mode();
        let result = self.apply_poll(event).await;
        let _ = self.ui.upgrade_in_event_loop(move |w| {
            w.set_poll_busy(false);
            if w.get_selected_room().as_str() != room || w.get_sheet().as_str() != mode {
                return;
            }
            match result {
                Ok(()) => {
                    w.set_poll_error("".into());
                    w.set_chosen("".into());
                    w.set_sheet("".into());
                }
                Err(e) => {
                    w.set_poll_error(e.to_string().into());
                }
            }
        });
    }

    async fn apply_poll(&mut self, event: Event) -> Result<()> {
        ensure!(
            self.page == Page::Chat
                && self.room.id == event.room()
                && !self.room.request
                && self.online(),
            "Reopen this conversation and try again."
        );
        match event {
            Event::Create { poll, .. } => {
                ensure!(
                    self.can_create_poll(),
                    "You need permission to send polls in this channel."
                );
                validate(&poll)?;
                if !self.preview {
                    return self
                        .service()?
                        .send_poll(&self.room.guild, &self.room.id, &poll)
                        .await;
                }
                let now = clock::now_ms();
                let answers = poll
                    .answers
                    .into_iter()
                    .enumerate()
                    .map(|(i, a)| PollAnswer {
                        id: (i + 1) as u64,
                        text: a.text,
                        ..Default::default()
                    })
                    .collect();
                let message = Message {
                    id: message::nonce(),
                    channel_id: self.room.id.clone(),
                    author_id: self.me().to_owned(),
                    author: crate::preview::LOCAL_NAME.into(),
                    poll: Some(Poll {
                        question: poll.question,
                        answers,
                        expiry: Some(now + u64::from(poll.hours) * 3_600_000),
                        multiselect: poll.multiselect,
                        counted: true,
                        finalized: false,
                    }),
                    ..Default::default()
                };
                self.put(message);
                self.tail += 1;
            }
            Event::Vote {
                item, mut answers, ..
            } => {
                let message = self
                    .posts
                    .iter()
                    .find(|p| p.id == item)
                    .context("This poll is no longer in the conversation.")?;
                let poll = message.poll.as_ref().context("This message has no poll.")?;
                let may_view = self.preview
                    || self.room.guild == PRIVATE
                    || self
                        .snapshot
                        .servers
                        .iter()
                        .find(|g| g.id == self.room.guild)
                        .and_then(|g| g.channel(&self.room.id))
                        .is_some_and(|c| c.marks.access != yapfast_discord::ready::Access::Hidden);
                ensure!(
                    may_view && !closed(poll),
                    "Voting is unavailable for this poll."
                );
                answers.sort_unstable();
                answers.dedup();
                ensure!(
                    (poll.multiselect || answers.len() <= 1)
                        && answers
                            .iter()
                            .all(|id| poll.answers.iter().any(|a| a.id == *id)),
                    "Choose a valid answer."
                );
                if !self.preview {
                    return self.service()?.vote(&self.room.id, &item, &answers).await;
                }
                let message = self
                    .posts
                    .iter_mut()
                    .find(|p| p.id == item)
                    .expect("validated above");
                let changes: Vec<_> =
                    poll_changes(message.poll.as_ref().expect("validated above"), &answers);
                for (id, selected) in changes {
                    message.vote(id, true, selected);
                }
            }
            Event::End { item, .. } => {
                let me = self.me().to_owned();
                let message = self
                    .posts
                    .iter_mut()
                    .find(|p| p.id == item)
                    .context("This poll is no longer in the conversation.")?;
                ensure!(
                    message.author_id == me,
                    "Only the author can end this poll."
                );
                let poll = message.poll.as_mut().context("This message has no poll.")?;
                ensure!(!closed(poll), "Voting has already ended.");
                if !self.preview {
                    return self.service()?.end_poll(&self.room.id, &item).await;
                }
                poll.finalized = true;
            }
        }
        self.samples
            .insert(self.room.id.clone(), self.posts.clone());
        Ok(())
    }
}

fn poll_changes(poll: &Poll, votes: &[u64]) -> Vec<(u64, bool)> {
    poll.answers
        .iter()
        .filter_map(|answer| {
            let now = votes.contains(&answer.id);
            (answer.me != now).then_some((answer.id, now))
        })
        .collect()
}
