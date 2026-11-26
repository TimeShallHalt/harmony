use super::*;
use std::path::{Path, PathBuf};
use yapfast_core::clip::conversion::{self, Format};

const VIDEOS: &[&str] = &["mp4", "m4v", "mov", "mkv", "webm"];
const AUDIO: &[&str] = &["mp3", "m4a", "wav", "ogg", "opus", "flac", "aac"];

fn format_for(path: &Path) -> Option<Format> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    if VIDEOS.contains(&extension.as_str()) {
        Some(Format::WebM)
    } else if AUDIO.contains(&extension.as_str()) {
        Some(Format::Ogg)
    } else {
        None
    }
}

async fn convert(
    input: PathBuf,
    output: PathBuf,
    format: Format,
    limit: u64,
) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    ensure!(
        output
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case(format.extension())),
        "Save this clip with the .{} extension.",
        format.extension()
    );
    let source = input.canonicalize()?;
    ensure!(
        output.canonicalize().ok().as_ref() != Some(&source),
        "Choose a different name to keep the original clip."
    );
    let directory = output
        .parent()
        .context("Choose a folder for the new clip")?;
    let temporary = tempfile::NamedTempFile::new_in(directory)?;
    conversion::convert(&source, temporary.reopen()?, format, limit).await?;
    let size = temporary.as_file().metadata()?.len();
    ensure!(
        size > 0 && size <= limit,
        "The conversion did not produce a file within this chat's limit."
    );
    temporary.as_file().sync_all()?;
    temporary.persist(&output)?;
    Ok(())
}

pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    let view = window.global::<ClipConversion>();
    view.set_available(true);
    let active: Rc<RefCell<Option<slint::JoinHandle<()>>>> = Rc::default();
    let work = active.clone();
    let weak = Rc::downgrade(adapter);
    let win = window.as_weak();
    view.on_choose(move || {
        let (Some(adapter), Some(window)) = (weak.upgrade(), win.upgrade()) else {
            return;
        };
        let a = adapter.borrow();
        let view = window.global::<ClipConversion>();
        let state = &a.app.state;
        if view.get_busy() || state.preview || !state.can_attach() {
            return;
        }
        let Some(room) = state.selected_room() else {
            return;
        };
        let id = room.id.clone();
        let user = state.discord_user.clone();
        let limit = room.upload_limit.min(conversion::MAX_OUTPUT);
        let runtime = a.runtime.clone();
        let weak = Rc::downgrade(&adapter);
        let win = window.as_weak();
        view.set_busy(true);
        let task = slint::spawn_local(async move {
            let result = async {
                let Some(input) = rfd::AsyncFileDialog::new()
                    .set_title("Choose a video or audio clip")
                    .add_filter("Video", VIDEOS)
                    .add_filter("Audio", AUDIO)
                    .pick_file()
                    .await
                else {
                    return Ok(None);
                };
                let format = format_for(input.path())
                    .ok_or_else(|| anyhow::anyhow!("Choose a supported video or audio clip."))?;
                let source = input.path().to_path_buf();
                let stem = source.file_stem().unwrap_or_default().to_string_lossy();
                let Some(output) = rfd::AsyncFileDialog::new()
                    .set_title("Save compressed clip")
                    .set_file_name(format!("{stem}-compressed.{}", format.extension()))
                    .add_filter("Compressed media", &[format.extension()])
                    .save_file()
                    .await
                else {
                    return Ok(None);
                };
                let destination = output.path().to_path_buf();
                let Some(window) = win.upgrade() else {
                    return Ok(None);
                };
                window.global::<ClipConversion>().set_converting(true);
                let file = destination.clone();
                let worker = runtime.spawn(convert(source, file, format, limit));
                worker.await??;
                Ok::<_, anyhow::Error>(Some(destination))
            }
            .await;
            let (Some(adapter), Some(window)) = (weak.upgrade(), win.upgrade()) else {
                return;
            };
            let view = window.global::<ClipConversion>();
            view.set_converting(false);
            view.set_busy(false);
            match result {
                Ok(Some(path)) => {
                    let mut a = adapter.borrow_mut();
                    if a.app.state.selected.as_deref() == Some(&id)
                        && a.app.state.discord_user == user
                        && a.app.state.can_attach()
                    {
                        a.action(&window, Action::Stage(vec![path]));
                    } else {
                        notice(&mut a, &window, "Compressed clip saved.".into());
                    }
                }
                Ok(None) => {}
                Err(error) => notice(&mut adapter.borrow_mut(), &window, error.to_string()),
            }
        });
        match task {
            Ok(task) => *work.borrow_mut() = Some(task),
            Err(_) => {
                view.set_busy(false);
                window.set_notice("Harmony could not start the clip converter.".into());
            }
        }
    });
    let win = window.as_weak();
    view.on_cancel(move || {
        if let Some(work) = active.borrow_mut().take() {
            work.abort();
        }
        if let Some(window) = win.upgrade() {
            let view = window.global::<ClipConversion>();
            view.set_busy(false);
            view.set_converting(false);
        }
    });
}

fn notice(a: &mut Adapter, window: &YapfastWindow, message: String) {
    a.app.state.notice = Some(message.clone());
    window.set_notice(message.into());
}
