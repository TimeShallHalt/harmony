use super::*;
use std::{fs::File, io::Read, path::Path};
use yapfast_core::camera_background;

struct Choosing(slint::Weak<YapfastWindow>);

impl Drop for Choosing {
    fn drop(&mut self) {
        if let Some(window) = self.0.upgrade() {
            window.global::<CameraBackground>().set_busy(false);
        }
    }
}

fn read(path: &Path) -> anyhow::Result<Vec<u8>> {
    let file = File::open(path)?;
    let info = file.metadata()?;
    anyhow::ensure!(
        info.is_file() && info.len() <= camera_background::MAX_BYTES,
        "Choose a regular picture file up to 16 MiB."
    );
    let mut bytes = Vec::new();
    file.take(camera_background::MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= camera_background::MAX_BYTES,
        "Choose a picture up to 16 MiB."
    );
    Ok(bytes)
}

pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    let path = adapter.borrow().root.join("camera-background");
    let settings = window.global::<CameraBackground>();
    settings.set_available(true);
    let current = match read(&path) {
        Ok(bytes) => match camera_background::prepare(&bytes) {
            Ok(image) => Some(image),
            Err(error) => {
                settings.set_status(format!("Couldn't open the saved background. {error}").into());
                None
            }
        },
        Err(error)
            if !error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            settings.set_status(format!("Couldn't read the saved background. {error}").into());
            None
        }
        _ => None,
    };
    settings.set_active(current.is_some());
    camera_background::set(current);

    let weak = Rc::downgrade(adapter);
    let window_weak = window.as_weak();
    let dest = path.clone();
    settings.on_choose(move || {
        let (Some(adapter), Some(window)) = (weak.upgrade(), window_weak.upgrade()) else {
            return;
        };
        let settings = window.global::<CameraBackground>();
        if settings.get_busy() {
            return;
        }
        settings.set_busy(true);
        let a = adapter.borrow();
        let runtime = a.runtime.clone();
        let weak = window.as_weak();
        let choosing = Choosing(weak.clone());
        let dest = dest.clone();
        drop(a);
        adapter.borrow_mut().dialog(async move {
            let _choosing = choosing;
            let chosen = rfd::AsyncFileDialog::new()
                .set_title("Choose a camera background")
                .add_filter("PNG and JPEG images", &["png", "jpg", "jpeg"])
                .pick_file()
                .await;
            let result = if let Some(file) = chosen {
                Some(
                    runtime
                        .spawn_blocking(move || {
                            let bytes = read(file.path())?;
                            let image = camera_background::prepare(&bytes)?;
                            Ok::<_, anyhow::Error>((bytes, image))
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r),
                )
            } else {
                None
            };
            if let Some(window) = weak.upgrade() {
                let settings = window.global::<CameraBackground>();
                let result = result.map(|result| {
                    result.and_then(|(bytes, image)| {
                        theme_pack::write_atomic(&dest, &bytes).map_err(anyhow::Error::msg)?;
                        Ok(image)
                    })
                });
                match result {
                    Some(Ok(image)) => {
                        camera_background::set(Some(image));
                        settings.set_active(true);
                        settings.set_status("Background saved. Test video to preview it.".into());
                    }
                    Some(Err(error)) => {
                        settings.set_status(format!("Couldn't use that image. {error}").into())
                    }
                    None => {}
                }
            }
        });
    });
    let weak = window.as_weak();
    settings.on_clear(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let settings = window.global::<CameraBackground>();
        if settings.get_busy() {
            return;
        }
        if let Err(error) = std::fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            settings.set_status(format!("Couldn't remove the saved background. {error}").into());
            return;
        }
        camera_background::set(None);
        settings.set_active(false);
        settings.set_status(Default::default());
    });
}
