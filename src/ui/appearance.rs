//! Appearance: choosing a theme, and making and editing the person's own.

use super::*;

impl Adapter {
    /// Shows the window in a theme, built in or the person's own, and keeps
    /// it chosen. False for an id Yapfast doesn't know, which changes nothing.
    pub(super) fn choose_theme(&mut self, window: &YapfastWindow, id: &str) -> bool {
        let Some((id, _)) =
            theme::lookup(id, &self.layout.custom_themes).map(|(id, theme)| (id.to_owned(), theme))
        else {
            return false;
        };
        let mut next = self.layout.clone();
        next.theme = Some(id);
        next.theme_pack = None;
        if let Err(error) = self.commit_appearance(window, next) {
            window.set_theme_pack_status(format!("Appearance unchanged: {error}").into());
            return false;
        }
        window.set_theme_pack_status(SharedString::default());
        true
    }

    /// Shows the window in another theme's colours, building again what
    /// carries the old mention colour in its markup: message rows, the
    /// topic, pins and search results.
    pub(super) fn set_theme(&mut self, window: &YapfastWindow, theme: &theme::Theme) {
        if theme.apply(window) {
            self.restyle_mentions();
            self.refresh(window);
        }
    }

    /// Marks for building again, at the next refresh, what carries the
    /// mention colour in its markup.
    pub(super) fn restyle_mentions(&mut self) {
        self.restyle = true;
        self.topic_shown = None;
        self.models.pins.clear();
        self.models.search_results.clear();
    }

    /// Opens the editor on a copy of the theme shown, which the person can
    /// swap for any other.
    pub(super) fn new_theme(&mut self, window: &YapfastWindow) {
        let current = window.get_theme_id();
        let current = self
            .layout
            .theme_pack
            .as_ref()
            .map(|saved| saved.pack.base.as_str())
            .unwrap_or(current.as_str());
        let starts: Vec<_> = theme::starts(&self.layout.custom_themes).collect();
        let index = starts
            .iter()
            .position(|start| start.id == current)
            .unwrap_or(0);
        let names: Vec<SharedString> = starts.iter().map(|start| start.name.into()).collect();
        let (base, start, name) = match &self.layout.theme_pack {
            Some(saved) => (
                theme::find(&saved.pack.base)
                    .unwrap_or(&theme::BUILT_IN[0])
                    .id,
                theme::pack_theme(&saved.pack),
                saved.pack.name.as_str(),
            ),
            None => (starts[index].base, starts[index].theme, starts[index].name),
        };
        window.set_theme_name(copy_name(name).into());
        window.set_theme_starts(ModelRc::new(VecModel::from(names)));
        window.set_theme_start(index as i32);
        self.open_theme_editor(window, theme::Editor::new(None, base, start));
    }

    /// Opens the editor on one of the person's own themes.
    pub(super) fn edit_theme(&mut self, window: &YapfastWindow, id: &str) {
        let Some(custom) = self
            .layout
            .custom_themes
            .iter()
            .find(|custom| custom.id == id)
        else {
            return;
        };
        window.set_theme_name(custom.name.as_str().into());
        let editor = theme::Editor::new(Some(custom.id.clone()), custom.base, custom.theme);
        self.open_theme_editor(window, editor);
    }

    fn open_theme_editor(&mut self, window: &YapfastWindow, editor: theme::Editor) {
        window.set_theme_editing_saved(editor.editing.is_some());
        window.set_theme_dark(editor.theme.dark);
        window.set_theme_colours(editor.rows());
        window.set_theme_can_save(true);
        window.set_theme_editor(true);
        window.global::<ClientStyle>().set_system24(editor.base == theme::SYSTEM24_ID);
        self.set_theme(window, &editor.theme);
        self.theme_editor = Some(editor);
    }

    /// The new theme starts again from another, and takes its name too
    /// unless the person named it.
    pub(super) fn theme_start_chosen(&mut self, window: &YapfastWindow, index: i32) {
        let custom = &self.layout.custom_themes;
        let start = |index: i32| {
            let index = usize::try_from(index).ok()?;
            theme::starts(custom)
                .nth(index)
                .map(|start| (start.base, start.theme, copy_name(start.name)))
        };
        let (Some(editor), Some((base, theme, name))) = (self.theme_editor.as_mut(), start(index))
        else {
            return;
        };
        let named = start(window.get_theme_start())
            .is_none_or(|(_, _, before)| before != window.get_theme_name().as_str());
        if !named {
            window.set_theme_name(name.into());
        }
        editor.restart(base, theme);
        window.set_theme_start(index);
        window.set_theme_dark(theme.dark);
        window.set_theme_can_save(true);
        window.global::<ClientStyle>().set_system24(base == theme::SYSTEM24_ID);
        self.set_theme(window, &theme);
    }

    /// A colour's field changed: the window shows it at once if it's a
    /// colour.
    pub(super) fn theme_colour_edited(&mut self, window: &YapfastWindow, index: i32, text: &str) {
        let Some(editor) = self.theme_editor.as_mut() else {
            return;
        };
        let changed = usize::try_from(index).is_ok_and(|index| editor.edit(index, text));
        window.set_theme_can_save(editor.valid());
        if changed {
            let theme = editor.theme;
            self.set_theme(window, &theme);
        }
    }

    pub(super) fn theme_dark_toggled(&mut self, window: &YapfastWindow, dark: bool) {
        let Some(editor) = self.theme_editor.as_mut() else {
            return;
        };
        editor.theme.dark = dark;
        let theme = editor.theme;
        self.set_theme(window, &theme);
    }

    /// Save the pack's palette in its snapshot, or a standalone custom palette.
    /// Persistence is checked before closing the editor or replacing saved state.
    pub(super) fn save_theme(&mut self, window: &YapfastWindow) {
        let Some(editor) = self.theme_editor.as_ref().filter(|editor| editor.valid()) else {
            return;
        };
        let name = match window.get_theme_name().trim() {
            "" => "My theme".to_owned(),
            name => name.to_owned(),
        };
        let mut next = self.layout.clone();
        if editor.editing.as_deref() == Some(theme::PACK_ID) {
            let Some(saved) = next.theme_pack.as_mut() else {
                return;
            };
            saved.pack.name = name;
            saved.pack.dark = Some(editor.theme.dark);
            saved.pack.colors = theme::pack_colors(&editor.theme);
            if let Err(error) = saved.validate() {
                window.set_theme_pack_status(format!("Not saved: {error}").into());
                return;
            }
        } else {
            let existing = editor.editing.as_ref().and_then(|id| {
                next.custom_themes
                    .iter_mut()
                    .find(|custom| custom.id == *id)
            });
            let id = if let Some(saved) = existing {
                saved.name = name;
                saved.theme = editor.theme;
                saved.base = editor.base;
                saved.id.clone()
            } else {
                let id = theme::new_id(&next.custom_themes);
                next.custom_themes.push(theme::Custom {
                    id: id.clone(),
                    name,
                    base: editor.base,
                    theme: editor.theme,
                });
                id
            };
            next.theme = Some(id);
            next.theme_pack = None;
        }
        if let Err(error) = self.commit_appearance(window, next) {
            window.set_theme_pack_status(format!("Not saved: {error}").into());
            return;
        }
        self.theme_editor = None;
        close_theme_editor(window);
        window.set_theme_pack_status("Appearance saved".into());
    }

    /// Closes the editor and shows the theme from before it opened.
    pub(super) fn cancel_theme(&mut self, window: &YapfastWindow) {
        if self.theme_editor.take().is_some() {
            close_theme_editor(window);
            self.restore_appearance(window);
        }
    }

    /// Delete a custom palette; deleting the active pack only resets its snapshot.
    pub(super) fn delete_theme(&mut self, window: &YapfastWindow) {
        let Some(editor) = self.theme_editor.as_ref() else {
            return;
        };
        let mut next = self.layout.clone();
        if editor.editing.as_deref() == Some(theme::PACK_ID) {
            next.theme_pack = None;
        } else {
            if let Some(id) = &editor.editing {
                next.custom_themes.retain(|custom| custom.id != *id);
            }
            if next.theme_pack.is_none() {
                let (id, _) = theme::chosen(next.theme.as_deref(), &next.custom_themes);
                next.theme = Some(id.to_owned());
            }
        }
        if let Err(error) = self.commit_appearance(window, next) {
            window.set_theme_pack_status(format!("Not deleted: {error}").into());
            return;
        }
        self.theme_editor = None;
        close_theme_editor(window);
    }

    fn restore_appearance(&mut self, window: &YapfastWindow) {
        let (_, chosen) = self.layout.selected_theme(window);
        let changed = chosen.apply(window);
        self.layout.apply_appearance(window);
        if changed {
            self.restyle_mentions();
            self.refresh(window);
        }
    }

    fn commit_appearance(
        &mut self,
        window: &YapfastWindow,
        next: Layout,
    ) -> std::result::Result<(), String> {
        next.try_write(&self.root)
            .map_err(|error| format!("Could not save appearance: {error}"))?;
        self.layout = next;
        self.restore_appearance(window);
        Ok(())
    }

    pub(super) fn import_theme_pack(
        &mut self,
        window: &YapfastWindow,
        path: &std::path::Path,
    ) -> std::result::Result<(), String> {
        if self.theme_editor.is_some() {
            return Err("Finish or cancel the color editor first".into());
        }
        let applied = theme_pack::Applied::load(path)?;
        let mut next = self.layout.clone();
        next.theme = Some(applied.pack.base.clone());
        next.theme_pack = Some(applied);
        self.commit_appearance(window, next)
    }

    pub(super) fn reload_theme_pack(
        &mut self,
        window: &YapfastWindow,
    ) -> std::result::Result<(), String> {
        let path = self
            .layout
            .theme_pack
            .as_ref()
            .and_then(|saved| saved.source.clone())
            .ok_or("Import a pack before reloading")?;
        self.import_theme_pack(window, &path)
    }

    pub(super) fn reset_theme_pack(
        &mut self,
        window: &YapfastWindow,
    ) -> std::result::Result<(), String> {
        if self.theme_editor.is_some() {
            return Err("Finish or cancel the color editor first".into());
        }
        let mut next = self.layout.clone();
        next.theme_pack = None;
        self.commit_appearance(window, next)
    }

    pub(super) fn reset_theme_logo(
        &mut self,
        window: &YapfastWindow,
    ) -> std::result::Result<(), String> {
        if self.theme_editor.is_some() {
            return Err("Finish or cancel the color editor first".into());
        }
        let mut next = self.layout.clone();
        let saved = next.theme_pack.as_mut().ok_or("No theme pack is applied")?;
        saved.pack.logo = None;
        saved.logo_png.clear();
        self.commit_appearance(window, next)
    }

    pub(super) fn edit_theme_pack(&mut self, window: &YapfastWindow) {
        let Some(saved) = &self.layout.theme_pack else {
            return;
        };
        let base = theme::find(&saved.pack.base)
            .unwrap_or(&theme::BUILT_IN[0])
            .id;
        let editor = theme::Editor::new(
            Some(theme::PACK_ID.into()),
            base,
            theme::pack_theme(&saved.pack),
        );
        window.set_theme_name(saved.pack.name.as_str().into());
        self.open_theme_editor(window, editor);
    }

    pub(super) fn export_theme_pack(
        &self,
        window: &YapfastWindow,
        path: &std::path::Path,
    ) -> std::result::Result<(), String> {
        if self.theme_editor.is_some() {
            return Err("Finish or cancel the color editor first".into());
        }
        let snapshot = if let Some(saved) = &self.layout.theme_pack {
            saved.clone()
        } else {
            let (id, chosen) = self.layout.selected_theme(window);
            let custom = self
                .layout
                .custom_themes
                .iter()
                .find(|theme| theme.id == id);
            let built_in = theme::find(&id).unwrap_or(&theme::BUILT_IN[0]);
            theme_pack::Applied {
                pack: theme_pack::ThemePack {
                    version: 1,
                    name: custom
                        .map(|theme| theme.name.clone())
                        .unwrap_or_else(|| built_in.name.to_owned()),
                    base: custom
                        .map(|theme| theme.base)
                        .unwrap_or(built_in.id)
                        .to_owned(),
                    dark: Some(chosen.dark),
                    colors: theme::pack_colors(&chosen),
                    layout: Default::default(),
                    typography: Default::default(),
                    motion: Default::default(),
                    logo: None,
                },
                source: None,
                logo_png: Vec::new(),
            }
        };
        snapshot.export(path)
    }
}

/// Native one-shot dialogs use the adapter's existing lifetime/cancellation guard.
pub(super) fn pack_dialog(adapter: &Rc<RefCell<Adapter>>, export: bool) {
    let weak = Rc::downgrade(adapter);
    adapter.borrow_mut().dialog(async move {
        let dialog = rfd::AsyncFileDialog::new()
            .add_filter("Harmony theme pack", &["harmony.json", "json"])
            .set_title(if export {
                "Export Harmony theme pack"
            } else {
                "Import Harmony theme pack"
            });
        let chosen = if export {
            dialog.set_file_name("theme.harmony.json").save_file().await
        } else {
            dialog.pick_file().await
        };
        let (Some(file), Some(adapter)) = (chosen, weak.upgrade()) else {
            return;
        };
        let window = adapter.borrow().window.upgrade();
        let Some(window) = window else {
            return;
        };
        let result = if export {
            adapter.borrow().export_theme_pack(&window, file.path())
        } else {
            adapter.borrow_mut().import_theme_pack(&window, file.path())
        };
        pack_status(
            &window,
            result,
            if export {
                "Theme pack exported"
            } else {
                "Theme pack imported and saved"
            },
        );
    });
}

pub(super) fn pack_status(
    window: &YapfastWindow,
    result: std::result::Result<(), String>,
    success: &str,
) {
    window.set_theme_pack_status(match result {
        Ok(()) => success.into(),
        Err(error) => format!("Appearance unchanged: {error}").into(),
    });
}

/// What a new theme copied from another is called until the person names it.
fn copy_name(name: &str) -> String {
    format!("{name} copy")
}

/// Drops the editor's models from the window, so they're held only while
/// it's open.
fn close_theme_editor(window: &YapfastWindow) {
    window.set_theme_editor(false);
    window.set_theme_colours(ModelRc::default());
    window.set_theme_starts(ModelRc::default());
}
