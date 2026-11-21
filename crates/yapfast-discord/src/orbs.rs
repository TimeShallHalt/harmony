//! A read-only view of Discord's consumer Quest and Shop responses. Missing
//! sections stay unavailable. Enrollment, reward claims and checkout belong
//! to the first-party pages that can present eligibility checks and terms.
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default)]
pub struct Overview {
    pub balance: Option<u64>,
    pub quests: Option<Vec<Quest>>,
    pub enrollment_blocked_until: Option<String>,
    pub current_available: bool,
    pub history_available: bool,
    pub items: Option<Vec<Item>>,
}

#[derive(Debug)]
pub struct Quest {
    pub id: u64,
    pub name: String,
    pub status: &'static str,
    pub progress: Vec<String>,
    pub expires: Option<String>,
    pub claim_by: Option<String>,
    pub reward: String,
    pub claimed_orbs: Option<u64>,
}

#[derive(Debug)]
pub struct Item {
    pub id: u64,
    pub name: String,
    pub summary: String,
    pub price: Option<u64>,
    pub owned: Option<bool>,
}

fn snowflake(value: &Value) -> Option<u64> {
    value.as_str()?.parse().ok().filter(|id| *id != 0)
}

fn date(value: &Value) -> Option<DateTime<Utc>> {
    value.as_str()?.parse().ok()
}

fn quest(
    id: u64,
    value: &Value,
    excluded: bool,
    claimed: Option<&Value>,
    now: DateTime<Utc>,
) -> Quest {
    let config = &value["config"];
    let user = &value["user_status"];
    let expires = date(&config["expires_at"]);
    let claim_end = date(&config["rewards_config"]["rewards_expire_at"]);
    let status = if claimed.is_some() || date(&user["claimed_at"]).is_some() {
        "Reward claimed"
    } else if date(&user["completed_at"]).is_some() {
        if claim_end.is_some_and(|t| t <= now) {
            "Claim window ended"
        } else {
            "Completed"
        }
    } else if excluded {
        "Ineligible"
    } else if expires.is_some_and(|t| t <= now) {
        "Expired"
    } else if date(&user["enrolled_at"]).is_some() {
        "Active"
    } else if date(&config["starts_at"]).is_some_and(|t| t > now) {
        "Upcoming"
    } else {
        "Available"
    };
    let tasks = &config["task_config_v2"]["tasks"];
    let mut progress = Vec::new();
    if let Some(reported) = user["progress"].as_object() {
        for (key, task) in reported {
            if let Some(count) = task["value"].as_u64() {
                let name = key.replace('_', " ").to_lowercase();
                progress.push(match tasks[key]["target"].as_u64() {
                    Some(target) => format!("{name}: {count}/{target}"),
                    None => format!("{name}: {count}"),
                });
            }
        }
    } else if let Some(seconds) = user["stream_progress_seconds"].as_u64() {
        progress.push(format!("{seconds} seconds streamed"));
    }
    let rewards = claimed
        .and_then(|q| q["config"]["rewards"].as_array())
        .or_else(|| config["rewards_config"]["rewards"].as_array())
        .or_else(|| config["rewards"].as_array());
    let reward = rewards
        .into_iter()
        .flatten()
        .filter_map(|r| {
            r["messages"]["name"]
                .as_str()
                .or_else(|| r["name"].as_str())
        })
        .collect::<Vec<_>>()
        .join(", ");
    Quest {
        id,
        name: config["messages"]["quest_name"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Quest {id}")),
        status,
        progress,
        expires: expires.map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string()),
        claim_by: claim_end
            .filter(|_| status != "Reward claimed")
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string()),
        reward,
        claimed_orbs: claimed
            .and_then(|q| q["user_status"]["orb_quantity_claimed"].as_u64())
            .or_else(|| user["orb_quantity_claimed"].as_u64()),
    }
}

fn items(shop: &Value, purchases: &Value, now: DateTime<Utc>) -> Option<Vec<Item>> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let owned = purchases
        .as_array()
        .or_else(|| purchases["purchases"].as_array())
        .map(|rows| {
            rows.iter()
                .filter(|p| date(&p["expires_at"]).is_none_or(|t| t > now))
                .filter_map(|p| snowflake(&p["sku_id"]))
                .collect::<HashSet<_>>()
        });
    for category in shop["categories"].as_array()? {
        for product in category["products"].as_array()? {
            let Some(id) = snowflake(&product["sku_id"]) else {
                continue;
            };
            if date(&product["unpublished_at"]).is_some_and(|t| t <= now) {
                continue;
            }
            // Only the standard, non-gift country price in Discord Orbs.
            // Discounts and the final amount are resolved again at checkout.
            let prices = &product["prices"]["0"]["country_prices"]["prices"];
            let price = prices
                .as_array()
                .into_iter()
                .flatten()
                .find(|p| p["currency"] == "discord_orb")
                .and_then(|p| p["amount"].as_u64());
            if (price.is_some() || category["is_orbs_exclusive"] == true)
                && let Some(name) = product["name"].as_str()
                && seen.insert(id)
            {
                // The collection only describes collectible ownership. A consumable
                // SKU (such as Nitro credit) has its own status in Discord's checkout.
                let owned = owned
                    .as_ref()
                    .filter(|_| product["type"].as_u64() != Some(3000))
                    .map(|ids| ids.contains(&id));
                let requirement = match product["premium_type"].as_u64() {
                    Some(1) => "Requires Nitro Classic",
                    Some(2) => "Requires Nitro",
                    Some(3) => "Requires Nitro Basic",
                    Some(4..) => "Check the membership requirement in Discord",
                    _ => "",
                };
                let summary = [product["summary"].as_str().unwrap_or_default(), requirement]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                result.push(Item {
                    id,
                    name: name.into(),
                    price,
                    owned,
                    summary,
                });
            }
        }
    }
    Some(result)
}

pub(crate) fn overview(
    current: &Value,
    history: &Value,
    wallet: &Value,
    shop: &Value,
    purchases: &Value,
    now: DateTime<Utc>,
) -> Overview {
    let current_rows = current["quests"].as_array();
    let historical_rows = history["quests"].as_array();
    let excluded: HashSet<_> = current["excluded_quests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|q| snowflake(&q["id"]))
        .collect();
    let claimed: HashMap<_, _> = historical_rows
        .into_iter()
        .flatten()
        .filter_map(|q| Some((snowflake(&q["id"])?, q)))
        .collect();
    let mut seen = HashSet::new();
    let quests = current_rows.or(historical_rows).map(|_| {
        current_rows
            .into_iter()
            .flatten()
            .chain(historical_rows.into_iter().flatten())
            .chain(current["excluded_quests"].as_array().into_iter().flatten())
            .filter(|q| q["preview"] != true)
            .filter_map(|q| {
                let id = snowflake(&q["id"])?;
                if !seen.insert(id) {
                    return None;
                }
                Some(quest(
                    id,
                    q,
                    excluded.contains(&id),
                    claimed.get(&id).copied(),
                    now,
                ))
            })
            .collect()
    });
    Overview {
        balance: wallet["balance"].as_u64(),
        quests,
        current_available: current_rows.is_some(),
        enrollment_blocked_until: date(&current["quest_enrollment_blocked_until"])
            .filter(|&t| t > now)
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string()),
        history_available: historical_rows.is_some(),
        items: items(shop, purchases, now),
    }
}
