//! Device-owned appearance. Only rendered ProfileData is overlaid. The account
//! snapshot and the Discord profile editor keep the profile received from Discord.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{Cursor, Read};
use std::path::Path;

const FONT_IDS: &[u8] = &[0, 3, 4, 6, 7, 8, 10, 12, 13, 14, 15, 16];
const MAX_PNG: usize = 2 * 1024 * 1024;
const MAX_FILE: usize = 2 * MAX_PNG.div_ceil(3) * 4 + 4096;

fn read_limited(file: std::fs::File, max: usize) -> Result<Vec<u8>, String> {
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > max as u64 {
        return Err(format!("Choose a regular file up to {} KiB.", max / 1024));
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err("The file is too large.".into());
    }
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    if bytes.len() > MAX_PNG {
        return Err("Choose a PNG up to 2 MiB.".into());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(24 * 1024 * 1024);
    let decoder = image::codecs::png::PngDecoder::with_limits(Cursor::new(bytes), limits)
        .map_err(|e| format!("Choose a static PNG up to 2048×1024. {e}"))?;
    if decoder.is_apng().map_err(|e| e.to_string())? {
        return Err("Choose a still PNG for this profile.".into());
    }
    image::DynamicImage::from_decoder(decoder)
        .map(|image| image.into_rgba8())
        .map_err(|e| e.to_string())
}

#[derive(Clone, PartialEq)]
struct Art {
    png: Rc<[u8]>,
    image: slint::Image,
}

impl Art {
    fn new(bytes: Vec<u8>, rgba: image::RgbaImage) -> Self {
        let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
        );
        Self {
            png: bytes.into(),
            image: slint::Image::from_rgba8(pixels),
        }
    }
}

impl Serialize for Art {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(&self.png))
    }
}

impl<'de> Deserialize<'de> for Art {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let encoded = <&str>::deserialize(deserializer)?;
        if encoded.len() > MAX_PNG.div_ceil(3) * 4 {
            return Err(serde::de::Error::custom("Profile PNG is too large"));
        }
        let bytes = STANDARD.decode(encoded).map_err(serde::de::Error::custom)?;
        let rgba = decode(&bytes).map_err(serde::de::Error::custom)?;
        Ok(Self::new(bytes, rgba))
    }
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Look {
    font: u8,
    top: String,
    bottom: String,
    banner: Option<Art>,
    decoration: Option<Art>,
}

fn color(text: &str) -> Option<slint::Color> {
    let text = text.trim();
    (text.len() == 7).then(|| theme::parse(text)).flatten()
}

impl Look {
    fn valid(&self) -> bool {
        [&self.top, &self.bottom]
            .iter()
            .all(|s| s.trim().is_empty() || color(s).is_some())
    }

    fn apply(&self, data: &mut ProfileData, cosmetics: yapfast_core::application::Cosmetics) {
        let family = name_style::family(self.font);
        if cosmetics.name_styles && !family.is_empty() {
            data.name_style = name_style::plain_font(family.into(), name_style::weight(self.font));
        }
        if let Some(banner) = &self.banner {
            data.banner = banner.image.clone();
            data.banner_moving = Default::default();
            data.banner_media = false;
        }
        if let Some(art) = &self.decoration
            && cosmetics.decorations
        {
            data.decoration = art.image.clone();
            data.decoration_url = Default::default();
            data.local_decoration = true;
        }
        let (top, bottom) = (color(&self.top), color(&self.bottom));
        if let Some(one) = top.or(bottom) {
            data.theme_primary = top.unwrap_or(if data.has_theme {
                data.theme_primary
            } else {
                one
            });
            data.theme_secondary = bottom.unwrap_or(if data.has_theme {
                data.theme_secondary
            } else {
                one
            });
            data.has_theme = true;
        }
    }
}

fn owner(state: &AppState) -> Option<&str> {
    if state.preview {
        return Some("preview");
    }
    state.discord_user.as_deref().filter(|id| discord_id(id))
}

fn discord_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedLook {
    version: u8,
    user: String,
    appearance: Look,
}

impl SharedLook {
    fn import(file: std::fs::File, subject: &str) -> Result<Self, String> {
        Self::parse(&read_limited(file, MAX_FILE)?, subject)
    }

    fn parse(bytes: &[u8], subject: &str) -> Result<Self, String> {
        let result: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("Invalid Harmony appearance file. {error}"))?;
        if result.version != 1 || result.user != subject || !discord_id(subject) {
            return Err("This appearance file does not match the open Discord profile.".into());
        }
        if !result.appearance.valid() || !FONT_IDS.contains(&result.appearance.font) {
            return Err("The appearance file has invalid colors or fonts.".into());
        }
        Ok(result)
    }
}

fn shared_path(root: &Path, account: &str, subject: &str) -> PathBuf {
    root.join("local-profiles")
        .join("shared")
        .join(account)
        .join(format!("{subject}.json"))
}

fn path(root: &Path, owner: &str) -> PathBuf {
    root.join("local-profiles").join(format!("{owner}.json"))
}

fn load(root: &Path, owner: &str) -> Result<Look, String> {
    let file = match std::fs::File::open(path(root, owner)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Look::default()),
        Err(error) => return Err(error.to_string()),
    };
    serde_json::from_slice(&read_limited(file, MAX_FILE)?).map_err(|e| e.to_string())
}

#[derive(Default)]
pub(super) struct Editor {
    owner: Option<String>,
    saved: Look,
    draft: Option<Look>,
    base: ProfileData,
    cosmetics: Option<yapfast_core::application::Cosmetics>,
    status: String,
    load_error: Option<String>,
    generation: u64,
    shared: VecDeque<(String, Option<Look>)>,
    account_revision: u64,
    pub(super) redraw_account: bool,
}

impl Editor {
    pub(super) fn sync(&mut self, root: &Path, state: &AppState, window: &YapfastWindow) {
        let owner = owner(state);
        if self.owner.as_deref() == owner {
            return;
        }
        if let Some(old) = self.owner.as_deref() {
            if window.get_profile().id == old {
                window.set_profile(Default::default());
            }
            if window.get_panel_profile().id == old {
                window.set_panel_profile(Default::default());
            }
        }
        let loaded = owner.map(|id| load(root, id)).transpose();
        self.load_error = loaded
            .as_ref()
            .err()
            .map(|e| format!("Couldn't read the local profile. {e}"));
        self.saved = loaded.ok().flatten().unwrap_or_default();
        self.owner = owner.map(str::to_owned);
        self.shared.clear();
        self.account_revision = self.account_revision.wrapping_add(1);
        self.base = Default::default();
        self.redraw_account = true;
        self.editing(
            window,
            window.get_settings_open() && window.get_settings_section() == 11,
            state.cosmetics,
        );
    }

    pub(super) fn editing(
        &mut self,
        window: &YapfastWindow,
        open: bool,
        cosmetics: yapfast_core::application::Cosmetics,
    ) {
        self.generation = self.generation.wrapping_add(1);
        self.draft = (open && self.owner.is_some()).then(|| self.saved.clone());
        self.status = self.load_error.clone().unwrap_or_default();
        if open && self.owner.is_none() {
            self.status =
                "Open a Discord account or Harmony’s local preview to edit a profile.".into();
        }
        self.show(window, cosmetics);
    }

    pub(super) fn decorate(&mut self, root: &Path, data: &mut ProfileData, state: &AppState) {
        let Some(account) = owner(state).filter(|id| Some(*id) == self.owner.as_deref()) else {
            return;
        };
        if data.id.as_str() == account {
            self.saved.apply(data, state.cosmetics);
        } else if discord_id(account) && discord_id(&data.id) {
            let entry = if let Some(index) = self
                .shared
                .iter()
                .position(|(id, _)| id == data.id.as_str())
            {
                self.shared.remove(index).unwrap()
            } else {
                let look = std::fs::File::open(shared_path(root, account, &data.id))
                    .ok()
                    .and_then(|file| SharedLook::import(file, &data.id).ok())
                    .map(|pack| pack.appearance);
                (data.id.to_string(), look)
            };
            if let Some(look) = &entry.1 {
                look.apply(data, state.cosmetics);
            }
            self.shared.push_front(entry);
            self.shared.truncate(4);
        }
    }

    pub(super) fn needs_account(
        &self,
        window: &YapfastWindow,
        state: &AppState,
        has_profile: bool,
    ) -> bool {
        self.redraw_account
            || self.cosmetics != Some(state.cosmetics)
            || (!has_profile
                && self.owner.is_some()
                && (self.base.name != window.get_account_name()
                    || self.base.handle != window.get_account_username()
                    || self.base.picture != window.get_account_picture()))
    }

    pub(super) fn account(
        &mut self,
        window: &YapfastWindow,
        state: &AppState,
        mut base: ProfileData,
    ) {
        let received = !base.id.is_empty();
        if !received && let Some(owner) = &self.owner {
            base.id = owner.as_str().into();
            base.own = true;
            base.name = window.get_account_name();
            base.handle = window.get_account_username();
            base.initial = window.get_account_initial();
            base.picture = window.get_account_picture();
            base.tint = window.get_account_tint();
        }
        self.base = base.clone();
        self.cosmetics = Some(state.cosmetics);
        if received || self.saved != Look::default() {
            if Some(base.id.as_str()) == self.owner.as_deref() {
                self.saved.apply(&mut base, state.cosmetics);
            }
            window.set_account_profile(base);
        } else {
            window.set_account_profile(Default::default());
        }
        self.redraw_account = false;
        if self.draft.is_some() {
            self.show(window, state.cosmetics);
        }
    }

    fn show(&self, window: &YapfastWindow, cosmetics: yapfast_core::application::Cosmetics) {
        let ui = window.global::<LocalProfile>();
        ui.set_available(self.draft.is_some());
        ui.set_can_export(
            self.draft.is_some()
                && self.owner.as_deref().is_some_and(discord_id)
                && self.saved != Look::default(),
        );
        let Some(draft) = &self.draft else {
            ui.set_preview(Default::default());
            ui.set_status(self.status.as_str().into());
            ui.set_can_save(false);
            ui.set_top(Default::default());
            ui.set_bottom(Default::default());
            ui.set_font(0);
            ui.set_has_banner(false);
            ui.set_has_decoration(false);
            return;
        };
        let mut preview = self.base.clone();
        draft.apply(&mut preview, cosmetics);
        ui.set_preview(preview);
        ui.set_top(draft.top.as_str().into());
        ui.set_bottom(draft.bottom.as_str().into());
        ui.set_font(
            FONT_IDS
                .iter()
                .position(|font| *font == draft.font)
                .unwrap_or(0) as i32,
        );
        ui.set_has_banner(draft.banner.is_some());
        ui.set_has_decoration(draft.decoration.is_some());
        ui.set_can_save(draft.valid() && (draft != &self.saved || self.load_error.is_some()));
        ui.set_status(
            if draft.valid() {
                self.status.as_str()
            } else {
                "Enter a six-digit hex color, or leave the field empty."
            }
            .into(),
        );
    }

    fn save(&mut self, root: &Path) -> Result<(), String> {
        let (Some(owner), Some(draft)) = (&self.owner, &self.draft) else {
            return Ok(());
        };
        if !draft.valid() {
            return Err("Check the profile colors.".into());
        }
        let file = path(root, owner);
        std::fs::create_dir_all(root.join("local-profiles")).map_err(|e| e.to_string())?;
        theme_pack::write_atomic(
            &file,
            &serde_json::to_vec(draft).map_err(|e| e.to_string())?,
        )?;
        self.saved = draft.clone();
        self.load_error = None;
        self.status = "Saved on this device.".into();
        self.redraw_account = true;
        Ok(())
    }
}

pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    let ui = window.global::<LocalProfile>();
    ui.set_fonts(ModelRc::from(
        FONT_IDS
            .iter()
            .map(|&id| {
                SharedString::from(if id == 0 {
                    "Profile default"
                } else {
                    name_style::family(id)
                })
            })
            .collect::<Vec<_>>()
            .as_slice(),
    ));
    macro_rules! bind {
        ($callback:ident, |$a:ident, $w:ident $(, $arg:ident)*| $body:block) => {{
            let (weak, win) = (Rc::downgrade(adapter), window.as_weak());
            ui.$callback(move |$($arg),*| {
                let (Some(owner), Some($w)) = (weak.upgrade(), win.upgrade()) else { return; };
                let mut borrowed = owner.borrow_mut();
                let $a = &mut *borrowed;
                $a.local_profile.sync(&$a.root, &$a.app.state, &$w);
                $body
            });
        }};
    }
    bind!(on_editing, |a, w, open| {
        a.local_profile.editing(&w, open, a.app.state.cosmetics);
    });
    bind!(on_choose_font, |a, w, index| {
        if let Some(&font) = usize::try_from(index).ok().and_then(|i| FONT_IDS.get(i))
            && let Some(draft) = &mut a.local_profile.draft
        {
            draft.font = font;
            a.local_profile.status.clear();
            a.local_profile.show(&w, a.app.state.cosmetics);
        }
    });
    bind!(on_edit_color, |a, w, top, text| {
        if let Some(draft) = &mut a.local_profile.draft {
            let field = if top {
                &mut draft.top
            } else {
                &mut draft.bottom
            };
            *field = text.chars().take(16).collect();
            a.local_profile.status.clear();
            a.local_profile.show(&w, a.app.state.cosmetics);
        }
    });
    bind!(on_clear_image, |a, w, banner| {
        if let Some(draft) = &mut a.local_profile.draft {
            if banner {
                draft.banner = None;
            } else {
                draft.decoration = None;
            }
            a.local_profile.generation = a.local_profile.generation.wrapping_add(1);
            a.local_profile.status.clear();
            a.local_profile.show(&w, a.app.state.cosmetics);
        }
    });
    bind!(on_reset, |a, w| {
        if a.local_profile.draft.is_some() {
            a.local_profile.draft = Some(Look::default());
            a.local_profile.generation = a.local_profile.generation.wrapping_add(1);
            a.local_profile.status.clear();
            a.local_profile.show(&w, a.app.state.cosmetics);
        }
    });
    bind!(on_save, |a, w| {
        if let Err(error) = a.local_profile.save(&a.root) {
            a.local_profile.status = format!("Couldn't save the local profile. {error}");
        }
        a.refresh_local_profile(&w);
        a.local_profile.show(&w, a.app.state.cosmetics);
    });
    bind!(on_export_look, |a, w| {
        let Some(user) = a.local_profile.owner.clone().filter(|id| discord_id(id)) else {
            return;
        };
        let pack = SharedLook {
            version: 1,
            user,
            appearance: a.local_profile.saved.clone(),
        };
        let weak = a.weak.clone();
        let win = w.as_weak();
        let revision = a.local_profile.account_revision;
        a.dialog(async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_title("Export saved Harmony appearance")
                .set_file_name("Profile.harmony-profile")
                .add_filter("Harmony profile", &["harmony-profile"])
                .save_file()
                .await
            else {
                return;
            };
            let (Some(adapter), Some(window)) = (weak.upgrade(), win.upgrade()) else {
                return;
            };
            let mut a = adapter.borrow_mut();
            if a.local_profile.account_revision != revision
                || owner(&a.app.state) != Some(pack.user.as_str())
            {
                return;
            }
            let result = serde_json::to_vec(&pack)
                .map_err(|e| e.to_string())
                .and_then(|bytes| theme_pack::write_atomic(file.path(), &bytes));
            a.local_profile.status = match result {
                Ok(()) => "Appearance exported. You can send the file as an attachment.".into(),
                Err(error) => format!("Couldn't export the appearance. {error}"),
            };
            a.local_profile.show(&window, a.app.state.cosmetics);
        });
    });
    bind!(on_pick_image, |a, w, banner| {
        if a.local_profile.draft.is_none() {
            return;
        }
        let weak = a.weak.clone();
        let win = w.as_weak();
        let generation = a.local_profile.generation;
        let runtime = a.runtime.clone();
        a.dialog(async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("Still PNG", &["png"])
                .set_title(if banner {
                    "Choose a local profile banner"
                } else {
                    "Choose a local avatar decoration"
                })
                .pick_file()
                .await
            else {
                return;
            };
            let result = runtime
                .spawn_blocking(move || {
                    let f = std::fs::File::open(file.path()).map_err(|e| e.to_string())?;
                    let bytes = read_limited(f, MAX_PNG)?;
                    let rgba = decode(&bytes)?;
                    Ok::<_, String>((bytes, rgba))
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r);
            if let (Some(a), Some(w)) = (weak.upgrade(), win.upgrade()) {
                a.borrow_mut()
                    .finish_local_image(&w, generation, banner, result);
            }
        });
    });
}

const IMPORT: &str = "import-shared-profile";
const REMOVE: &str = "remove-shared-profile";

fn shared_subject<'a>(state: &'a AppState, target: &'a str) -> Option<(&'a str, &'a str)> {
    let subject = target.strip_prefix(menus::PROFILE_MORE)?;
    let account = owner(state).filter(|id| discord_id(id))?;
    (discord_id(subject) && account != subject).then_some((account, subject))
}

pub(super) fn share_menu(a: &Adapter, target: &str) -> Vec<MenuRow> {
    let Some((account, subject)) = shared_subject(&a.app.state, target) else {
        return vec![];
    };
    let menu_row = |id: &str, text: &str| MenuRow {
        id: id.into(),
        text: text.into(),
        plain: true,
        ..Default::default()
    };
    let mut rows = vec![menu_row(IMPORT, "Import Harmony appearance…")];
    if shared_path(&a.root, account, subject).is_file() {
        rows.push(menu_row(REMOVE, "Remove shared appearance"));
    }
    rows
}

pub(super) fn choose_shared(
    a: &mut Adapter,
    window: &YapfastWindow,
    target: &str,
    id: &str,
) -> bool {
    if !matches!(id, IMPORT | REMOVE) {
        return false;
    }
    let Some((account, subject)) = shared_subject(&a.app.state, target) else {
        return true;
    };
    let path = shared_path(&a.root, account, subject);
    if id == REMOVE {
        let result = std::fs::remove_file(&path);
        if result.is_ok() {
            a.local_profile.shared.retain(|(id, _)| id != subject);
            a.refresh_local_profile(window);
        }
        notice(
            a,
            window,
            result.map_or_else(
                |error| format!("Couldn't remove the shared appearance. {error}"),
                |_| "Shared appearance removed.".into(),
            ),
        );
        return true;
    }
    let (account, subject) = (account.to_owned(), subject.to_owned());
    let weak = a.weak.clone();
    let win = window.as_weak();
    let runtime = a.runtime.clone();
    let revision = a.local_profile.account_revision;
    a.dialog(async move {
        let Some(file) = rfd::AsyncFileDialog::new()
            .set_title("Import this person's Harmony appearance")
            .add_filter("Harmony profile", &["harmony-profile"])
            .pick_file()
            .await
        else {
            return;
        };
        let bytes = runtime
            .spawn_blocking(move || {
                let file = std::fs::File::open(file.path()).map_err(|e| e.to_string())?;
                read_limited(file, MAX_FILE)
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
        let (Some(adapter), Some(window)) = (weak.upgrade(), win.upgrade()) else {
            return;
        };
        let mut a = adapter.borrow_mut();
        if owner(&a.app.state) != Some(account.as_str())
            || a.local_profile.account_revision != revision
        {
            return;
        }
        let result = bytes.and_then(|bytes| {
            let pack = SharedLook::parse(&bytes, &subject)?;
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            theme_pack::write_atomic(&path, &bytes)?;
            Ok(pack.appearance)
        });
        let message = match result {
            Ok(look) => {
                a.local_profile.shared.retain(|(id, _)| id != &subject);
                a.local_profile.shared.push_front((subject, Some(look)));
                a.local_profile.shared.truncate(4);
                a.refresh_local_profile(&window);
                "Shared appearance saved for this profile.".into()
            }
            Err(error) => error,
        };
        notice(&mut a, &window, message);
    });
    true
}

fn notice(a: &mut Adapter, window: &YapfastWindow, message: String) {
    a.app.state.notice = Some(message.clone());
    window.set_notice(message.into());
}

impl Adapter {
    fn finish_local_image(
        &mut self,
        window: &YapfastWindow,
        generation: u64,
        banner: bool,
        result: Result<(Vec<u8>, image::RgbaImage), String>,
    ) {
        let editor = &mut self.local_profile;
        if editor.generation != generation || editor.owner.as_deref() != owner(&self.app.state) {
            return;
        }
        let Some(draft) = &mut editor.draft else {
            return;
        };
        match result {
            Ok((bytes, rgba)) => {
                let slot = if banner {
                    &mut draft.banner
                } else {
                    &mut draft.decoration
                };
                *slot = Some(Art::new(bytes, rgba));
                editor.status.clear();
            }
            Err(error) => editor.status = error,
        }
        editor.show(window, self.app.state.cosmetics);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yapfast_core::chat::{DiscordCommand, OwnProfile, Profile};

    fn setup(root: &Path, id: &str) -> (YapfastWindow, Rc<RefCell<Adapter>>) {
        let w = YapfastWindow::new().unwrap();
        let a = Adapter::new(Arc::new(BackgroundRuntime::new().unwrap()), root.into(), &w).unwrap();
        bind_callbacks(&w, &a);
        let profile = Profile {
            id: id.into(),
            name: "River".into(),
            ..Default::default()
        };
        {
            let mut a = a.borrow_mut();
            a.app.state.discord_user = Some(id.into());
            a.app.state.account_profile = Some(Arc::new(profile.clone()));
            a.app.state.profile = Some(ProfileView {
                profile,
                ..Default::default()
            });
            a.refresh(&w);
        }
        w.set_settings_section(11);
        w.set_settings_open(true);
        i_slint_core::properties::ChangeTracker::run_change_handlers();
        assert!(w.global::<LocalProfile>().get_available());
        (w, a)
    }

    fn png() -> (Vec<u8>, image::RgbaImage) {
        let rgba = image::RgbaImage::from_pixel(3, 2, image::Rgba([70, 20, 100, 255]));
        let mut out = Cursor::new(Vec::new());
        rgba.write_to(&mut out, image::ImageFormat::Png).unwrap();
        (out.into_inner(), rgba)
    }

    #[test]
    fn shared_appearance_is_bound_to_the_recipient_and_subject() {
        i_slint_backend_testing::init_no_event_loop();
        let dir = tempfile::tempdir().unwrap();
        let (window, a) = setup(dir.path(), "42");
        let (bytes, art) = png();
        let pack = SharedLook {
            version: 1,
            user: "43".into(),
            appearance: Look {
                top: "#193266".into(),
                banner: Some(Art::new(bytes, art)),
                ..Default::default()
            },
        };
        let data = serde_json::to_vec(&pack).unwrap();
        assert!(SharedLook::parse(&data, "44").is_err());
        let dest = shared_path(dir.path(), "42", "43");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        theme_pack::write_atomic(&dest, &data).unwrap();
        let mut a = a.borrow_mut();
        let mut profile = ProfileData {
            id: "43".into(),
            ..Default::default()
        };
        let state = a.app.state.discord_user.clone();
        assert_eq!(state.as_deref(), Some("42"));
        {
            let Adapter {
                local_profile,
                root,
                app,
                ..
            } = &mut *a;
            local_profile.decorate(root, &mut profile, &app.state);
        }
        assert_eq!(profile.banner.size().width, 3);
        assert_eq!(profile.theme_primary, color("#193266").unwrap());
        a.app.state.discord_user = Some("44".into());
        let mut profile = ProfileData {
            id: "43".into(),
            ..Default::default()
        };
        let Adapter {
            local_profile,
            root,
            app,
            ..
        } = &mut *a;
        local_profile.sync(root, &app.state, &window);
        local_profile.decorate(root, &mut profile, &app.state);
        assert!(!profile.has_theme);
    }

    #[test]
    fn local_profile_preview_save_reload_and_discord_payload_are_isolated() {
        i_slint_backend_testing::init_no_event_loop();
        let dir = tempfile::tempdir().unwrap();
        let (w, a) = setup(dir.path(), "42");
        let ui = w.global::<LocalProfile>();
        let before = a.borrow().backend_command_count;
        ui.invoke_choose_font(2);
        ui.invoke_edit_color(true, "#193266".into());
        ui.invoke_edit_color(false, "#aa3311".into());
        let generation = a.borrow().local_profile.generation;
        for banner in [true, false] {
            a.borrow_mut()
                .finish_local_image(&w, generation, banner, Ok(png()));
        }
        assert_eq!(
            ui.get_preview().name_style.row_data(0).unwrap().family,
            "Jellybean"
        );
        assert!(ui.get_preview().local_decoration);
        assert!(!w.get_account_profile().has_theme);
        assert!(!path(dir.path(), "42").exists());
        ui.invoke_save();
        assert_eq!(a.borrow().backend_command_count, before);
        assert_eq!(w.get_profile().theme_secondary, color("#aa3311").unwrap());
        assert!(w.get_account_profile().local_decoration);
        assert_eq!(w.get_account_profile().banner.size().width, 3);
        assert!(!w.get_account_profile().banner_media);
        ui.invoke_reset();
        assert!(!ui.get_preview().has_theme);
        w.set_settings_open(false);
        i_slint_core::properties::ChangeTracker::run_change_handlers();
        assert!(w.get_account_profile().has_theme);

        // The independent, paid-capable Discord editor sends only a subsequent real name edit.
        let mut a = a.borrow_mut();
        let received = a
            .app
            .state
            .account_profile
            .as_ref()
            .unwrap()
            .as_ref()
            .clone();
        assert!(received.theme.is_none() && received.banner.is_none() && received.style.is_none());
        a.app.dispatch(Action::EditProfile);
        a.app.apply(Event::OwnProfile {
            server: None,
            result: Ok(OwnProfile {
                name: "River".into(),
                fallback: received,
                ..Default::default()
            }),
        });
        a.app.state.profile_editor.as_mut().unwrap().premium = 2;
        assert!(
            a.app
                .state
                .profile_editor
                .as_ref()
                .unwrap()
                .change()
                .is_none()
        );
        a.app.dispatch(Action::WriteProfile {
            field: yapfast_core::application::ProfileText::Name,
            text: "New name".into(),
        });
        let effects = a.app.dispatch(Action::SaveProfile);
        let [Effect::Backend(Command::Discord(DiscordCommand::SaveProfile { change, .. }))] =
            effects.as_slice()
        else {
            panic!("expected one Discord profile save")
        };
        assert_eq!(change.name.as_deref(), Some("New name"));
        assert!(
            change.avatar.is_none()
                && change.banner.is_none()
                && change.theme.is_none()
                && change.accent.is_none()
                && change.name_style.is_none()
                && change.primary_server.is_none()
        );
        drop(a);
        let (w2, _a2) = setup(dir.path(), "42");
        let loaded = w2.global::<LocalProfile>().get_preview();
        assert_eq!(loaded.name_style.row_data(0).unwrap().family, "Jellybean");
        assert_eq!(loaded.theme_primary, color("#193266").unwrap());
        assert_eq!(loaded.banner.size().width, 3);
        assert_eq!(loaded.decoration.size().height, 2);
    }

    #[test]
    fn local_profile_rejects_stale_picks_invalid_colors_and_failed_saves() {
        i_slint_backend_testing::init_no_event_loop();
        let dir = tempfile::tempdir().unwrap();
        let (w, a) = setup(dir.path(), "42");
        let ui = w.global::<LocalProfile>();
        ui.invoke_edit_color(true, "#ffffff".into());
        ui.invoke_save();
        let saved = std::fs::read(path(dir.path(), "42")).unwrap();
        ui.invoke_edit_color(false, "#zzzzzz".into());
        assert!(!ui.get_can_save());
        ui.invoke_save();
        assert_eq!(std::fs::read(path(dir.path(), "42")).unwrap(), saved);
        let stale = a.borrow().local_profile.generation;
        {
            let mut a = a.borrow_mut();
            a.app.state.discord_user = Some("73".into());
            a.refresh(&w);
            a.finish_local_image(&w, stale, true, Ok(png()));
        }
        assert!(!ui.get_has_banner() && !ui.get_preview().has_theme);
        assert!(!w.get_profile().has_theme);
        std::fs::create_dir(path(dir.path(), "73")).unwrap();
        ui.invoke_edit_color(false, "#c0ffee".into());
        ui.invoke_save();
        assert!(ui.get_status().contains("Couldn't save"));
        assert!(!w.get_account_profile().has_theme);
        assert!(ui.get_can_save());
        assert!(decode(b"not an image").is_err());
    }
}
