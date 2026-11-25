slint::include_modules!();
use slint::ComponentHandle;
use std::sync::Arc;

mod call;
#[allow(dead_code)]
#[path = "../../../crates/yapfast-core/src/clock.rs"]
mod clock;
#[path = "../../../crates/yapfast-core/src/discord_link.rs"]
mod discord_link;
mod emoji;
#[allow(dead_code)]
#[path = "../../../crates/yapfast-core/src/links.rs"]
mod links;
mod pictures;
mod platform;
mod preview;
#[allow(dead_code)]
#[path = "../../../crates/yapfast-core/src/runtime.rs"]
mod runtime;
mod shell;
#[allow(dead_code)]
#[path = "../../../crates/yapfast-core/src/spoiler.rs"]
mod spoiler;
mod view;

#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    if let Err(e) = launch(app) {
        eprintln!("Harmony startup failed: {e:#}");
    }
}
fn launch(app: slint::android::AndroidApp) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let runtime = Arc::new(runtime::BackgroundRuntime::new()?);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let platform = platform::Platform::new(&app, tx.clone())?;
    if !cfg!(feature = "fixture") {
        if let Some(dir) = app.internal_data_path() {
            yapfast_discord::cache::keep_in(dir.join("discord-account.json"));
        }
    }
    slint::android::init(app)?;
    let window = MobileWindow::new()?;
    emoji::bind(&window);
    shell::bind(&window, tx.clone());
    let task = runtime.spawn(shell::run(
        window.as_weak(),
        platform,
        runtime.clone(),
        tx,
        rx,
    ));
    let result = window.run();
    // Wait for Session and its Android devices to drop before shutting down Tokio on this thread.
    task.abort_handle().abort();
    let _ = runtime.block_on(task);
    drop(runtime);
    platform::detach();
    result?;
    Ok(())
}
