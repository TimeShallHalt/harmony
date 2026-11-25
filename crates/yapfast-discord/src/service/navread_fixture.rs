//! Store/socket/task-free handle for production history publication acceptance.
//! No service::start, cache, token, own_activity, gateway or HTTP request runs.
use super::*;
pub fn publication_handle(snapshot: Snapshot) -> Discord {
    let (requests, _discarded) = mpsc::unbounded_channel();
    let (_, snapshots) = watch::channel(Arc::new(snapshot));
    let (live, _) = broadcast::channel(LIVE_BACKLOG);
    Discord {
        requests,
        snapshots,
        live,
        api: Arc::new(Api::anonymous().expect("anonymous HTTP client construction only")),
        names: Arc::default(),
        profiles: Arc::default(),
        tag_servers: Arc::default(),
        feed_place: Arc::default(),
        notes: Notes::default(),
        effects: Arc::default(),
        frames: Arc::default(),
        listed_threads: Arc::default(),
        new_dms: Arc::new(Mutex::new(limits::Window::new(NEW_DMS, NEW_DM_SPAN))),
        application_icons: Arc::default(),
        mfa: Arc::default(),
        default_sounds: Arc::default(),
        // Publication fixtures deliberately create neither a gateway watcher
        // nor any other background task.
        commands: commands::Kept::default(),
        autocompletes: interactions::Autocompletes::default(),
        status_writes: Arc::default(),
    }
}
