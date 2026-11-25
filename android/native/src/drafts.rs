use crate::platform::Vault;
use anyhow::{Context, Result};
use std::{collections::HashMap, time::Duration};
use tokio::time::Instant;
use yapfast_discord::token::SecretStore;

// Only text and its reply target can be restored independently of Android's temporary picker files.
type Entries = HashMap<String, (String, Option<String>)>;

#[derive(Default)]
pub(super) struct Drafts {
    key: String,
    pub(super) entries: Entries,
    pub(super) deadline: Option<Instant>,
}

impl Drafts {
    pub(super) fn select(&mut self, account: &str) -> Result<bool> {
        if account.is_empty() {
            return Ok(false);
        }
        let key = format!("message-drafts/{account}");
        if self.key == key {
            return Ok(false);
        }
        self.flush()?;
        let entries = Vault
            .read(&key)?
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .context("Could not restore message drafts")?
            .unwrap_or_default();
        *self = Self {
            key,
            entries,
            deadline: None,
        };
        Ok(true)
    }

    pub(super) fn update(&mut self, room: &str, body: &str, reply: Option<&String>) {
        if self.key.is_empty() || room.is_empty() {
            return;
        }
        if body.is_empty() && reply.is_none() {
            if self.entries.remove(room).is_none() {
                return;
            }
        } else {
            if self
                .entries
                .get(room)
                .is_some_and(|(s, r)| s == body && r.as_ref() == reply)
            {
                return;
            }
            self.entries
                .insert(room.into(), (body.into(), reply.cloned()));
        }
        self.deadline = Some(Instant::now() + Duration::from_millis(300));
    }

    pub(super) fn flush(&mut self) -> Result<()> {
        if self.deadline.is_some() {
            let value = if self.entries.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&self.entries)?)
            };
            if let Err(error) = Vault.write(&self.key, value.as_deref()) {
                self.deadline = Some(Instant::now() + Duration::from_secs(15));
                return Err(error).context("Could not save message drafts");
            }
            self.deadline = None;
        }
        Ok(())
    }

    pub(super) fn forget(&mut self) -> Result<()> {
        if !self.key.is_empty() {
            Vault.write(&self.key, None)?;
        }
        *self = Self::default();
        Ok(())
    }
}
