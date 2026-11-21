// Shared framing and normalized Rich Presence parser, on both platforms.
const MAX_PACKET: usize = 64 * 1024;
const WAIT: Duration = Duration::from_secs(15);
async fn read<S: AsyncRead + Unpin>(stream: &mut S) -> Result<(u32, Vec<u8>)> {
    // An idle established client costs no polling. Once any byte arrives,
    // the complete header/body must finish within WAIT, including slowloris.
    let first = stream.read_u8().await?;
    tokio::time::timeout(WAIT, async {
        let mut opcode = [first, 0, 0, 0];
        stream.read_exact(&mut opcode[1..]).await?;
        let size = stream.read_u32_le().await? as usize;
        ensure!(size <= MAX_PACKET, "RPC packet too large");
        let mut payload = vec![0; size];
        stream.read_exact(&mut payload).await?;
        Ok((u32::from_le_bytes(opcode), payload))
    })
    .await?
}

async fn write<S: AsyncWrite + Unpin>(stream: &mut S, opcode: u32, payload: &[u8]) -> Result<()> {
    ensure!(payload.len() <= MAX_PACKET, "RPC packet too large");
    tokio::time::timeout(WAIT, async {
        stream.write_u32_le(opcode).await?;
        stream.write_u32_le(payload.len() as u32).await?;
        stream.write_all(payload).await
    })
    .await??;
    Ok(())
}

async fn send<S: AsyncWrite + Unpin>(stream: &mut S, value: Value) -> Result<()> {
    write(stream, 1, &serde_json::to_vec(&value)?).await
}

fn snowflake(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|id| id > 0)
}

type Assets = HashMap<String, String>;

fn web_address(text: &str, longest: usize) -> bool {
    text.len() <= longest && (text.starts_with("https://") || text.starts_with("http://"))
}

/// The media proxy's names for image addresses, as `mp:` asset keys.
fn external_lookup(response: &Value) -> Result<Assets> {
    Ok(response
        .as_array()
        .context("invalid external assets")?
        .iter()
        .filter_map(|asset| {
            Some((
                asset["url"].as_str()?.to_owned(),
                format!("mp:{}", asset["external_asset_path"].as_str()?),
            ))
        })
        .collect())
}

fn asset_lookup(response: &Value) -> Result<Assets> {
    Ok(response
        .as_array()
        .context("invalid application assets")?
        .iter()
        .filter_map(|asset| {
            Some((
                asset["name"].as_str()?.to_owned(),
                asset["id"].as_str()?.to_owned(),
            ))
        })
        .collect())
}

/// RPC uses seconds for timestamps and asset names; the Gateway needs
/// milliseconds and asset ids. Select fields so callers cannot inject a
/// custom status, another application id, or internal gateway metadata.
fn normalize(
    input: &Value,
    client: &str,
    name: &str,
    assets: Option<&Assets>,
    addresses: &Assets,
) -> Result<Value> {
    ensure!(input.is_object(), "activity must be an object");
    let kind = match input.get("type") {
        None => 0,
        Some(value) => value.as_u64().context("invalid activity type")?,
    };
    ensure!(matches!(kind, 0 | 2 | 3 | 5), "unsupported activity type");
    let mut output = json!({"name":name,"type":kind,"application_id":client});
    for key in [
        "state",
        "details",
        "state_url",
        "details_url",
        "party",
        "secrets",
        "instance",
        "status_display_type",
    ] {
        if let Some(value) = input.get(key).filter(|v| !v.is_null()) {
            output[key] = value.clone();
        }
    }
    for key in ["state", "details"] {
        if let Some(value) = output.get(key) {
            ensure!(
                value.as_str().is_some_and(|s| s.chars().count() <= 128),
                "invalid {key}"
            );
        }
    }
    for key in ["state_url", "details_url"] {
        if let Some(value) = output.get(key) {
            ensure!(
                value
                    .as_str()
                    .is_some_and(|s| s.len() <= 256
                        && (s.starts_with("https://") || s.starts_with("http://"))),
                "invalid {key}"
            );
        }
    }
    if let Some(value) = output.get("instance") {
        ensure!(value.is_boolean(), "instance must be a boolean");
    }
    if let Some(value) = output.get("status_display_type") {
        ensure!(
            value.as_u64().is_some_and(|n| n <= 2),
            "invalid status_display_type"
        );
    }
    if let Some(party) = output.get("party") {
        ensure!(party.is_object(), "party must be an object");
        if let Some(id) = party.get("id") {
            ensure!(
                id.as_str().is_some_and(|s| s.chars().count() <= 128),
                "invalid party id"
            );
        }
        if let Some(size) = party.get("size") {
            ensure!(
                size.as_array().is_some_and(|s| s.len() == 2
                    && s[0]
                        .as_u64()
                        .zip(s[1].as_u64())
                        .is_some_and(|(current, max)| max > 0 && current <= max)),
                "invalid party size"
            );
        }
    }
    if let Some(secrets) = output.get("secrets") {
        ensure!(secrets.is_object(), "secrets must be an object");
        for key in ["join", "spectate", "match"] {
            if let Some(value) = secrets.get(key) {
                ensure!(
                    value.as_str().is_some_and(|s| s.chars().count() <= 128),
                    "invalid {key} secret"
                );
            }
        }
    }
    if let Some(timestamps) = input.get("timestamps").filter(|v| !v.is_null()) {
        ensure!(timestamps.is_object(), "invalid timestamps");
        let mut converted = json!({});
        for key in ["start", "end"] {
            if let Some(value) = timestamps.get(key) {
                let seconds = value
                    .as_u64()
                    .context("timestamp must be a positive integer")?;
                // Some modern SDKs already send milliseconds.
                converted[key] = json!(if seconds < 1_000_000_000_000 {
                    seconds.checked_mul(1000).context("timestamp overflow")?
                } else {
                    seconds
                });
            }
        }
        output["timestamps"] = converted;
    }
    if let Some(images) = input.get("assets").filter(|v| !v.is_null()) {
        ensure!(images.is_object(), "invalid assets");
        let mut converted = json!({});
        for key in ["large_image", "small_image", "large_text", "small_text"] {
            if let Some(value) = images.get(key).filter(|v| !v.is_null()) {
                let text = value.as_str().context("asset must be text")?;
                let text = if key.ends_with("_image") && web_address(text, usize::MAX) {
                    addresses
                        .get(text)
                        .map(String::as_str)
                        .context("Discord didn't take the image's address")?
                } else if key.ends_with("_image") {
                    assets
                        .and_then(|lookup| lookup.get(text))
                        .map(String::as_str)
                        .or_else(|| snowflake(text).then_some(text))
                        .context("unknown application asset")?
                } else {
                    text
                };
                converted[key] = json!(text);
            }
        }
        output["assets"] = converted;
    }
    if let Some(buttons) = input.get("buttons").filter(|v| !v.is_null()) {
        let buttons = buttons.as_array().context("buttons must be an array")?;
        ensure!(buttons.len() <= 2, "at most two buttons");
        let mut labels = Vec::new();
        let mut urls = Vec::new();
        for button in buttons {
            let label = button["label"].as_str().context("button needs a label")?;
            let url = button["url"].as_str().context("button needs a URL")?;
            ensure!(
                !label.is_empty() && label.chars().count() <= 32,
                "invalid button label"
            );
            ensure!(
                url.len() <= 512 && (url.starts_with("https://") || url.starts_with("http://")),
                "invalid button URL"
            );
            labels.push(label);
            urls.push(url);
        }
        output["buttons"] = json!(labels);
        output["metadata"] = json!({"button_urls":urls});
    }
    // Leave room for a custom status and the rest of the Gateway envelope.
    ensure!(
        serde_json::to_vec(&output)?.len() <= 3072,
        "activity is too large"
    );
    Ok(output)
}
