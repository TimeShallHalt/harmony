//! Opening Discord's own protocol links in Harmony: a current-user
//! protocol registration on Windows, and the person's default
//! applications on Linux. Harmony's own `harmony:` scheme is registered by
//! its installer or desktop entry; this claims `discord:` only when
//! Settings says to, and gives the previous handler back when turned off.

use std::path::Path;

/// Whether this platform can claim Discord's protocol links.
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux"));

/// Whether Discord protocol links open in this Harmony.
pub fn discord_enabled(root: &Path) -> bool {
    platform::enabled(root)
}

/// Claims Discord protocol links for this Harmony, or gives them back.
pub fn set_discord_enabled(root: &Path, enabled: bool) -> std::io::Result<()> {
    platform::set(root, enabled)
}

/// Points a claimed registration at this Harmony after its folder moved
/// or an update replaced it. A handler someone else chose since is left.
pub fn refresh(root: &Path) {
    if let Err(error) = platform::refresh(root) {
        tracing::warn!(%error, "could not update Discord link handling");
    }
}

/// This Harmony as a command: itself, its data folder when it isn't the
/// usual one, then the link the desktop hands it.
#[cfg(windows)]
fn command(root: &Path) -> std::io::Result<Vec<String>> {
    use crate::autostart::path_text;
    let mut command = vec![path_text(std::env::current_exe()?)?];
    let usual = yapfast_core::default_directory().ok();
    if usual.as_deref() != Some(root) {
        command.extend([
            "--data-dir".to_owned(),
            path_text(std::path::absolute(root)?)?,
        ]);
    }
    Ok(command)
}

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(not(any(windows, target_os = "linux")))]
use none as platform;
#[cfg(windows)]
use windows as platform;
// src/notify.rs registers Harmony's toasts with it.
#[cfg(windows)]
pub(crate) use windows::set_string;

#[cfg(not(any(windows, target_os = "linux")))]
mod none {
    use std::path::Path;

    pub fn enabled(_: &Path) -> bool {
        false
    }

    pub fn set(_: &Path, _: bool) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "opening Discord links here isn't supported on this platform",
        ))
    }

    pub fn refresh(_: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

// BEGIN pure Windows registration identity (also exercised without Cargo).
#[cfg(any(windows, test))]
mod identity {
    /// Registration ownership includes the executable AND the effective root.
    pub(super) struct Context<'a> {
        pub program: &'a str,
        pub root: &'a str,
        pub usual: Option<&'a str>,
    }

    fn path(text: &str) -> Option<String> {
        let text = text.replace('/', "\\").to_ascii_lowercase();
        let absolute = text.as_bytes().get(1..3) == Some(b":\\")
            && text.as_bytes()[0].is_ascii_alphabetic()
            || text.starts_with("\\\\");
        if !absolute || text.contains(['\0', '%', '"', '*', '?']) {
            return None;
        }
        let parts: Vec<_> = text.split('\\').filter(|part| !part.is_empty()).collect();
        if parts.iter().any(|part| matches!(*part, "." | ".."))
            || text.starts_with("\\\\") && parts.len() < 2
        {
            return None;
        }
        Some(text.trim_end_matches('\\').to_owned())
    }

    /// Strict Windows command matcher, not shell/whitespace splitting. Accepts
    /// backslash/quote escaping but rejects ambiguous program quoting, unmatched
    /// quotes, NULs, controls and commands exceeding Windows' bound. No native
    /// pointers or unbounded pointer-string reads are involved.
    fn split(line: &str) -> Option<(Vec<String>, usize)> {
        if line.is_empty()
            || line.len() > 131_068
            || line.encode_utf16().count() > 32_767
            || line.chars().any(|c| c.is_control() && c != '\t')
        {
            return None;
        }
        let program_end = if let Some(rest) = line.strip_prefix('"') {
            let end = rest.find('"')? + 2;
            if !line[end..].is_empty() && !line[end..].starts_with([' ', '\t']) {
                return None;
            }
            end
        } else {
            line.find([' ', '\t']).unwrap_or(line.len())
        };
        let program = line[..program_end].trim_matches('"');
        if program.is_empty() || program.contains('"') {
            return None;
        }
        let mut args = vec![program.to_owned()];
        let mut chars = line[program_end..].chars().peekable();
        while chars.peek().is_some() {
            while chars.peek().is_some_and(|c| matches!(c, ' ' | '\t')) {
                chars.next();
            }
            if chars.peek().is_none() {
                break;
            }
            let mut argument = String::new();
            let mut quoted = false;
            while let Some(&character) = chars.peek() {
                if !quoted && matches!(character, ' ' | '\t') {
                    break;
                }
                chars.next();
                match character {
                    '\\' => {
                        let mut count = 1;
                        while chars.peek() == Some(&'\\') {
                            chars.next();
                            count += 1;
                        }
                        if chars.peek() == Some(&'"') {
                            argument.extend(std::iter::repeat_n('\\', count / 2));
                            chars.next();
                            if count % 2 == 1 {
                                argument.push('"');
                            } else {
                                if quoted && chars.peek() == Some(&'"') {
                                    return None;
                                }
                                quoted = !quoted;
                            }
                        } else {
                            argument.extend(std::iter::repeat_n('\\', count));
                        }
                    }
                    '"' => {
                        // Adjacent quotes inside a quoted argument have subtly
                        // different legacy Windows meanings. They are not in
                        // our generated grammar; fail closed, never guess.
                        if quoted && chars.peek() == Some(&'"') {
                            return None;
                        }
                        quoted = !quoted;
                    }
                    _ => argument.push(character),
                }
            }
            if quoted {
                return None;
            }
            args.push(argument);
        }
        Some((args, program_end))
    }

    /// Only this exact launch context may retarget an old executable. Original
    /// argument bytes, including spacing and quoting, are kept verbatim.
    pub(super) fn retarget(line: &str, context: &Context<'_>) -> Option<String> {
        let (args, program_end) = split(line)?;
        let program = path(context.program)?;
        let registered = path(args.first()?)?;
        let (parent, filename) = program.rsplit_once('\\')?;
        if filename != "harmony.exe"
            || registered != program && registered != format!("{parent}\\yapfast.exe")
        {
            return None;
        }
        let mut options = &args[1..];
        if options.last().map(String::as_str) != Some("%1") {
            return None;
        }
        options = &options[..options.len() - 1];
        if options.last().map(String::as_str) == Some("--open-link") {
            options = &options[..options.len() - 1];
        }
        // Only the historical protocol grammar proves ownership. Unknown
        // options, argument mentions and duplicate roots fail closed.
        let data = match options {
            [] => context.usual?,
            [option, root] if option == "--data-dir" => root,
            _ => return None,
        };
        if path(data)? != path(context.root)? {
            return None;
        }
        Some(format!(
            "{}{}",
            crate::autostart::windows_program(context.program),
            &line[program_end..]
        ))
    }

    /// Absent commands never produce updates. Refresh is not an opt-in.
    pub(super) fn refresh_plan(
        commands: [Option<&str>; 3],
        context: &Context<'_>,
    ) -> Vec<(usize, String)> {
        commands
            .into_iter()
            .enumerate()
            .filter_map(|(index, current)| {
                let current = current?;
                let updated = retarget(current, context)?;
                (updated != current).then_some((index, updated))
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        const PROGRAM: &str = r"C:\Program Files\Harmony\Harmony.exe";
        const ROOT: &str = r"C:\Users\A B\Harmony\data";
        fn context() -> Context<'static> {
            Context {
                program: PROGRAM,
                root: ROOT,
                usual: Some(ROOT),
            }
        }

        #[test]
        fn same_executable_different_data_directory_is_not_owned() {
            assert!(retarget(r#""C:\Program Files\Harmony\Harmony.exe" --data-dir "C:\isolated" --open-link "%1""#, &context()).is_none());
            let isolated = Context {
                root: r"C:\isolated",
                ..context()
            };
            assert!(
                retarget(
                    r#""C:\Program Files\Harmony\Harmony.exe" --open-link "%1""#,
                    &isolated
                )
                .is_none()
            );
        }

        #[test]
        fn same_install_legacy_executable_preserves_argument_bytes() {
            let line = r#""C:\Program Files\Harmony\yapfast.exe"  --data-dir "C:/Users/A B/Harmony/data"   --open-link "%1""#;
            assert_eq!(
                retarget(line, &context()).unwrap(),
                line.replacen("yapfast.exe", "Harmony.exe", 1)
            );
            assert!(
                retarget(r#""C:\Program Files\Harmony\yapfast.exe" "%1""#, &context()).is_some()
            );
        }

        #[test]
        fn stock_other_install_and_argument_mentions_are_not_owned() {
            for line in [
                r#""C:\Program Files\Yapfast\yapfast.exe" "%1""#,
                r#""D:\Harmony\Harmony.exe" "%1""#,
                r#""C:\other.exe" "C:\Program Files\Harmony\Harmony.exe" "%1""#,
                r#""C:\Program Files\Harmony\Harmony.exe" --preview "%1""#,
                r#""C:\Program Files\Harmony\Harmony.exe" --open-link "not%1""#,
                r#""C:\Program Files\Harmony\Harmony.exe" --data-dir "C:\other" --data-dir "C:\Users\A B\Harmony\data" "%1""#,
            ] {
                assert!(retarget(line, &context()).is_none(), "{line}");
            }
        }

        #[test]
        fn refresh_never_creates_an_absent_scheme() {
            let old = r#""C:\Program Files\Harmony\yapfast.exe" --open-link "%1""#;
            let plan = refresh_plan([Some(old), None, Some(old)], &context());
            assert_eq!(
                plan.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
                vec![0, 2]
            );
            assert!(refresh_plan([None, None, None], &context()).is_empty());
            let stock = r#""C:\Yapfast\yapfast.exe" "%1""#;
            assert!(refresh_plan([Some(stock), None, Some(stock)], &context()).is_empty());
        }

        #[test]
        fn strict_windows_parser_handles_spaces_escapes_and_bounds() {
            for argument in [
                "",
                r"C:\space folder\",
                "say \"hello\"",
                "tabs\there",
                r"plain\path",
            ] {
                let line = format!(
                    "\"C:\\program.exe\" {}",
                    crate::autostart::windows_argument(argument)
                );
                assert_eq!(
                    split(&line).unwrap().0,
                    vec![r"C:\program.exe".to_owned(), argument.to_owned()]
                );
            }
            for line in [
                "",
                "\"C:\\program.exe\"bad %1",
                "\"C:\\program.exe\" \"unterminated",
                "\"x\" \0",
                "\"x\" \"C:\\da\"\"ta\"",
            ] {
                assert!(split(line).is_none());
            }
            assert!(split(&"x".repeat(32_768)).is_none());
        }
    }
}
// END pure Windows registration identity.

#[cfg(windows)]
mod windows {
    use std::{io, path::Path};

    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
            REG_VALUE_TYPE, RegCloseKey, RegDeleteKeyValueW, RegDeleteKeyW, RegOpenKeyExW,
            RegQueryValueExW, RegSetKeyValueW, RegSetValueExW,
        },
    };

    const PROTOCOL: &str = r"Software\Classes\discord";
    const HARMONY_PROTOCOL: &str = r"Software\Classes\harmony";
    const HARMONY_COMMAND: &str = r"Software\Classes\harmony\shell\open\command";
    const HARMONY_ICON: &str = r"Software\Classes\harmony\DefaultIcon";
    const LEGACY_COMMAND: &str = r"Software\Classes\yapfast\shell\open\command";
    const COMMAND: &str = r"Software\Classes\discord\shell\open\command";
    // Keep the existing rollback record so an already-claimed Discord handler can be restored.
    const BACKUP: &str = r"Software\Yapfast\DiscordProtocol";
    const PREVIOUS_COMMAND: &str = "PreviousCommand";
    const HAD_COMMAND: &str = "HadCommand";
    const HAD_URL_PROTOCOL: &str = "HadUrlProtocol";
    const OWNER_COMMAND: &str = "OwnerCommand";

    struct Launch {
        program: String,
        root: String,
        usual: Option<String>,
    }

    impl Launch {
        fn new(root: &Path) -> io::Result<Self> {
            use crate::autostart::path_text;
            Ok(Self {
                program: path_text(std::env::current_exe()?)?,
                root: path_text(std::path::absolute(root)?)?,
                usual: yapfast_core::default_directory()
                    .ok()
                    .and_then(|p| path_text(p).ok()),
            })
        }

        fn context(&self) -> super::identity::Context<'_> {
            super::identity::Context {
                program: &self.program,
                root: &self.root,
                usual: self.usual.as_deref(),
            }
        }

        fn owns(&self, line: &str) -> bool {
            super::identity::retarget(line, &self.context()).is_some()
        }
    }

    pub fn enabled(root: &Path) -> bool {
        let Some(current) = query_string(COMMAND, None) else {
            return false;
        };
        Launch::new(root).is_ok_and(|launch| launch.owns(&current))
    }

    pub fn set(root: &Path, enabled: bool) -> io::Result<()> {
        let launch = Launch::new(root)?;
        let ours = command_line(&super::command(root)?);
        let current = read_value(COMMAND, None)?;
        let current_owned = current
            .as_ref()
            .and_then(Value::text)
            .is_some_and(|line| launch.owns(&line));
        let had = read_value(BACKUP, Some(HAD_COMMAND))?;
        let owner = read_value(BACKUP, Some(OWNER_COMMAND))?;
        // Old builds have no owner field. Their backup is attributable only
        // while the exact executable/data-root handler is still selected.
        let backup_owned = match owner.as_ref() {
            Some(owner) => owner.text().is_some_and(|line| launch.owns(&line)),
            None => current_owned,
        };
        let has_backup = had.is_some()
            || owner.is_some()
            || read_value(BACKUP, Some(PREVIOUS_COMMAND))?.is_some()
            || read_value(BACKUP, Some(HAD_URL_PROTOCOL))?.is_some();
        if has_backup && !backup_owned {
            return Err(io::Error::other(
                "Discord's saved handler belongs to another or unidentifiable instance; left unchanged",
            ));
        }
        if enabled {
            if !has_backup && !current_owned {
                let url = read_value(PROTOCOL, Some("URL Protocol"))?;
                if let Some(previous) = current.as_ref() {
                    set_raw_value(BACKUP, Some(PREVIOUS_COMMAND), previous)?;
                }
                set_string(
                    BACKUP,
                    Some(HAD_COMMAND),
                    if current.is_some() { "1" } else { "0" },
                )?;
                set_string(
                    BACKUP,
                    Some(HAD_URL_PROTOCOL),
                    if url.is_some() { "1" } else { "0" },
                )?;
            }
            if has_backup || !current_owned {
                set_string(BACKUP, Some(OWNER_COMMAND), &ours)?;
            }
            register_harmony(root, &ours)?;
            if read_value(PROTOCOL, Some("URL Protocol"))?.is_none() {
                set_string(PROTOCOL, Some("URL Protocol"), "")?;
            }
            return set_string(COMMAND, None, &ours);
        }
        if !current_owned || !has_backup {
            // Do not delete an unattributable/other instance's rollback record
            // merely because this instance's Settings was turned off.
            return Ok(());
        }
        match had.and_then(|value| value.text()).as_deref() {
            Some("1") => {
                let previous = read_value(BACKUP, Some(PREVIOUS_COMMAND))?.ok_or_else(|| {
                    io::Error::other("saved Discord handler is missing; left unchanged")
                })?;
                // Restore exact bytes AND registry type, not decoded text.
                set_raw_value(COMMAND, None, &previous)?;
            }
            Some("0") => delete_value(COMMAND, None)?,
            _ => {
                return Err(io::Error::other(
                    "saved Discord handler is incomplete; left unchanged",
                ));
            }
        }
        if query_string(BACKUP, Some(HAD_URL_PROTOCOL)).as_deref() == Some("0") {
            delete_value(PROTOCOL, Some("URL Protocol"))?;
        }
        match unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(BACKUP).as_ptr()) } {
            ERROR_FILE_NOT_FOUND => Ok(()),
            status => checked(status),
        }
    }

    pub fn refresh(root: &Path) -> io::Result<()> {
        let launch = Launch::new(root)?;
        let keys = [COMMAND, HARMONY_COMMAND, LEGACY_COMMAND];
        let values = [
            read_value(keys[0], None)?,
            read_value(keys[1], None)?,
            read_value(keys[2], None)?,
        ];
        let text = values
            .each_ref()
            .map(|value| value.as_ref().and_then(Value::text));
        for (index, updated) in super::identity::refresh_plan(
            text.each_ref().map(|line| line.as_deref()),
            &launch.context(),
        ) {
            let previous = values[index]
                .as_ref()
                .expect("refresh only includes existing values");
            // Recheck the exact target. Never create a scheme, marker, alias,
            // icon or backup during an ordinary launch.
            replace_existing(keys[index], previous, &updated)?;
        }
        Ok(())
    }

    /// Registers Harmony's own scheme for this copy, as the installer
    /// does, so a portable copy can be handed `harmony:` links too.
    fn register_harmony(root: &Path, ours: &str) -> io::Result<()> {
        set_string(HARMONY_PROTOCOL, None, "URL:Harmony Protocol")?;
        set_string(HARMONY_PROTOCOL, Some("URL Protocol"), "")?;
        let program = super::command(root)?
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("Harmony's own path is unavailable"))?;
        set_string(
            HARMONY_ICON,
            None,
            &format!("{},0", crate::autostart::windows_argument(&program)),
        )?;
        set_string(HARMONY_COMMAND, None, ours)
    }

    /// The registered command line, with the link last, after
    /// `--open-link`. Windows puts the link in as it comes, quotes
    /// included, so whatever it splits into follows the marker, where
    /// Harmony reads no option. The program comes first, quoted as Windows
    /// names one.
    fn command_line(command: &[String]) -> String {
        let mut line: Vec<String> = command
            .iter()
            .enumerate()
            .map(|(at, argument)| match at {
                0 => crate::autostart::windows_program(argument),
                _ => crate::autostart::windows_argument(argument),
            })
            .collect();
        line.extend([crate::OPEN_LINK.to_owned(), "\"%1\"".to_owned()]);
        line.join(" ")
    }

    pub(crate) fn set_string(key: &str, name: Option<&str>, value: &str) -> io::Result<()> {
        set_value(key, name, value, REG_SZ)
    }

    /// Writes a string value of `kind`, REG_SZ or REG_EXPAND_SZ.
    fn set_value(
        key: &str,
        name: Option<&str>,
        value: &str,
        kind: REG_VALUE_TYPE,
    ) -> io::Result<()> {
        let bytes = wide(value).into_iter().flat_map(u16::to_le_bytes).collect();
        set_raw_value(key, name, &Value { bytes, kind })
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Value {
        bytes: Vec<u8>,
        kind: REG_VALUE_TYPE,
    }

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    fn replace_existing(key: &str, previous: &Value, text: &str) -> io::Result<()> {
        let mut handle = std::ptr::null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(key).as_ptr(),
                0,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                &mut handle,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        checked(status)?;
        let handle = Key(handle);
        if read_open_value(handle.0, std::ptr::null())?.as_ref() == Some(previous) {
            let bytes: Vec<_> = wide(text).into_iter().flat_map(u16::to_le_bytes).collect();
            // Write only through an existing-key handle, not a create-key API.
            // If concurrently deleted, this registry object cannot recreate
            // the scheme. Read and write address the same object.
            checked(unsafe {
                RegSetValueExW(
                    handle.0,
                    std::ptr::null(),
                    0,
                    previous.kind,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                )
            })?;
        }
        Ok(())
    }

    impl Value {
        fn text(&self) -> Option<String> {
            if !matches!(self.kind, REG_SZ | REG_EXPAND_SZ) || self.bytes.len() % 2 != 0 {
                return None;
            }
            let mut units: Vec<_> = self
                .bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            if units.pop() != Some(0) || units.contains(&0) {
                return None;
            }
            String::from_utf16(&units).ok()
        }
    }

    fn set_raw_value(key: &str, name: Option<&str>, value: &Value) -> io::Result<()> {
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                wide(key).as_ptr(),
                name.map(wide)
                    .as_ref()
                    .map_or(std::ptr::null(), Vec::as_ptr),
                value.kind,
                value.bytes.as_ptr().cast(),
                value.bytes.len() as u32,
            )
        };
        checked(status)
    }

    fn delete_value(key: &str, name: Option<&str>) -> io::Result<()> {
        let status = unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                wide(key).as_ptr(),
                name.map(wide)
                    .as_ref()
                    .map_or(std::ptr::null(), Vec::as_ptr),
            )
        };
        match status {
            ERROR_FILE_NOT_FOUND => Ok(()),
            status => checked(status),
        }
    }

    /// A string value under `key`, unexpanded, by its name or the key's
    /// default value when it has none.
    fn query_string(key: &str, name: Option<&str>) -> Option<String> {
        read_value(key, name)
            .ok()
            .flatten()
            .and_then(|value| value.text())
    }

    /// Read raw bytes without RegGetValue's string-terminator repair or
    /// environment expansion. Missing values and read failures are distinct.
    fn read_value(key: &str, name: Option<&str>) -> io::Result<Option<Value>> {
        let (key, name) = (wide(key), name.map(wide));
        let name = name.as_ref().map_or(std::ptr::null(), Vec::as_ptr);
        let mut handle = std::ptr::null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut handle,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        checked(status)?;
        let handle = Key(handle);
        read_open_value(handle.0, name)
    }

    /// The name points to the caller's bounded, NUL-terminated wide buffer.
    fn read_open_value(handle: HKEY, name: *const u16) -> io::Result<Option<Value>> {
        let mut size = 0u32;
        let mut kind = 0;
        let status = unsafe {
            RegQueryValueExW(
                handle,
                name,
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        checked(status)?;
        if size > 1_048_576 {
            return Err(io::Error::other(
                "registry value exceeds the registration read bound",
            ));
        }
        let mut data = vec![0u8; size as usize];
        let status = unsafe {
            RegQueryValueExW(
                handle,
                name,
                std::ptr::null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut size,
            )
        };
        checked(status)?;
        data.truncate(size as usize);
        Ok(Some(Value { bytes: data, kind }))
    }

    fn checked(status: WIN32_ERROR) -> io::Result<()> {
        match status {
            ERROR_SUCCESS => Ok(()),
            status => Err(io::Error::from_raw_os_error(status as i32)),
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn registers_an_explicit_link_marker() {
            assert_eq!(
                command_line(&[r"C:\Apps\Harmony\yapfast.exe".into()]),
                r#""C:\Apps\Harmony\yapfast.exe" --open-link "%1""#
            );
        }

        /// Writes the current user's registry: run it under Wine with a
        /// throwaway WINEPREFIX, not on a Windows account.
        #[test]
        #[ignore = "writes the current user's Classes key"]
        fn claims_and_restores_the_discord_protocol() {
            let previous = query_string(COMMAND, None);
            set(Path::new("."), true).unwrap();
            assert!(enabled(Path::new(".")));
            set(Path::new("."), false).unwrap();
            assert!(!enabled(Path::new(".")));
            assert_eq!(query_string(COMMAND, None), previous);
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    //! The person's own `mimeapps.list` is the setting: it's on while its
    //! default applications name Harmony first for `discord:`. Harmony's
    //! desktop entry declares only `harmony:`, so without a Discord app
    //! installed the desktop doesn't fall back to Harmony for Discord's
    //! links; gio and xdg-open follow a default all the same.

    use std::{
        io::{self, Write},
        path::{Path, PathBuf},
    };

    use crate::desktop_entry;

    /// What the switch claims: Discord's links, and Harmony's own, which
    /// another app may have been chosen for.
    const SCHEMES: [&str; 2] = ["x-scheme-handler/discord", "x-scheme-handler/harmony"];
    const LEGACY_SCHEME: &str = "x-scheme-handler/yapfast";
    const DEFAULTS: &str = "[Default Applications]";
    /// In the data folder: each scheme's line in `mimeapps.list` before
    /// Harmony claimed it, as it was, or nothing when there was none.
    const PREVIOUS: &str = "discord-protocol-previous";

    fn desktop() -> String {
        format!("{}.desktop", crate::icon::APP_ID)
    }

    pub fn enabled(root: &Path) -> bool {
        list().is_some_and(|list| enabled_with_legacy(&list, legacy_desktop(root).is_some()))
    }

    pub fn set(root: &Path, enabled: bool) -> io::Result<()> {
        let list = list().ok_or_else(|| io::Error::other("no home folder"))?;
        if enabled {
            desktop_entry::install();
            if !desktop_exists() {
                return Err(io::Error::other("Harmony's desktop entry isn't installed"));
            }
        }
        apply_with_legacy(
            &list,
            &root.join(PREVIOUS),
            enabled,
            legacy_desktop(root).is_some(),
        )
    }

    pub fn refresh(root: &Path) -> io::Result<()> {
        // Retain a proven owned legacy ID/default instead of silently claiming
        // a new MIME alias. Only its active launch command may be retargeted.
        if let Some((path, current)) = legacy_desktop(root)
            && let Some(updated) = desktop_entry::identity::refreshed(&current, &launch(root)?)
        {
            desktop_entry::write(&path, updated.as_bytes())?;
        }
        Ok(())
    }

    fn launch(root: &Path) -> io::Result<Vec<String>> {
        let exe = std::env::current_exe()?;
        let program = desktop_entry::appimage(&exe).unwrap_or(exe);
        let mut command = vec![crate::autostart::path_text(program)?];
        if yapfast_core::default_directory().ok().as_deref() != Some(root) {
            command.extend([
                "--data-dir".to_owned(),
                crate::autostart::path_text(std::path::absolute(root)?)?,
            ]);
        }
        command.extend(["--open-link".to_owned(), "%u".to_owned()]);
        Ok(command)
    }

    /// The first XDG entry wins, even if stock/unreadable: never skip a stock
    /// user entry to find an older fork entry in a lower-priority system folder.
    fn legacy_desktop(root: &Path) -> Option<(PathBuf, String)> {
        let command = launch(root).ok()?;
        for folder in application_folders() {
            let path = folder
                .join("applications")
                .join(format!("{}.desktop", desktop_entry::LEGACY_ID));
            match std::fs::read_to_string(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => return None,
                Ok(text) => {
                    return (desktop_entry::identity::owned(&text, &command)
                        && desktop_entry::identity::active(&text))
                    .then_some((path, text));
                }
            }
        }
        None
    }

    #[cfg(test)]
    fn enabled_in(list: &Path) -> bool {
        enabled_with_legacy(list, false)
    }

    fn enabled_with_legacy(list: &Path, legacy_owned: bool) -> bool {
        std::fs::read_to_string(list).is_ok_and(|text| {
            default_line(&text, SCHEMES[0])
                .is_some_and(|line| names_harmony(line) || legacy_owned && names_legacy(line))
        })
    }

    /// Claims the schemes in `list`, keeping their lines in `previous`, or
    /// puts those lines back where Harmony still has them.
    #[cfg(test)]
    fn apply(list: &Path, previous: &Path, enabled: bool) -> io::Result<()> {
        apply_with_legacy(list, previous, enabled, false)
    }

    fn apply_with_legacy(
        list: &Path,
        previous: &Path,
        enabled: bool,
        legacy_owned: bool,
    ) -> io::Result<()> {
        let current = match std::fs::read_to_string(list) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            text => text?,
        };
        let mut text = current.clone();
        let mut restored = false;
        if enabled {
            // Kept from the first claim on: a line naming Harmony is no
            // one else's to give back.
            if !previous.try_exists()? {
                let lines = SCHEMES.map(|scheme| {
                    default_line(&current, scheme)
                        .filter(|line| !(names_harmony(line) || legacy_owned && names_legacy(line)))
                });
                let kept: String = lines
                    .into_iter()
                    .flatten()
                    .map(|line| format!("{line}\n"))
                    .collect();
                desktop_entry::write(previous, kept.as_bytes())?;
            }
            for scheme in SCHEMES {
                text = with_line(&text, scheme, Some(&format!("{scheme}={};", desktop())));
            }
        } else {
            let kept = match std::fs::read_to_string(previous) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
                result => result?,
            };
            for scheme in SCHEMES.into_iter().chain([LEGACY_SCHEME]) {
                if default_line(&text, scheme)
                    .is_some_and(|line| names_harmony(line) || legacy_owned && names_legacy(line))
                {
                    let line = kept.lines().find(|line| key(line) == Some(scheme));
                    text = with_line(&text, scheme, line);
                    restored = true;
                }
            }
        }
        if text != current {
            write_list(list, &text)?;
        }
        // An unproven legacy registration might still need this rollback file
        // for a manual migration. An off/no-op must not discard it.
        let unproven_legacy = !legacy_owned
            && [SCHEMES[0], LEGACY_SCHEME]
                .into_iter()
                .any(|scheme| default_line(&current, scheme).is_some_and(names_legacy));
        if !enabled && restored && !unproven_legacy {
            match std::fs::remove_file(previous) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                result => result?,
            }
        }
        Ok(())
    }

    /// Whether a default line names Harmony first.
    fn names_harmony(line: &str) -> bool {
        line.split_once('=')
            .and_then(|(_, value)| value.split(';').next())
            .is_some_and(|first| first.trim() == desktop())
    }

    fn names_legacy(line: &str) -> bool {
        line.split_once('=')
            .and_then(|(_, value)| value.split(';').next())
            .is_some_and(|first| first.trim() == format!("{}.desktop", desktop_entry::LEGACY_ID))
    }

    /// The key a `key=value` line sets.
    fn key(line: &str) -> Option<&str> {
        line.split_once('=').map(|(key, _)| key.trim())
    }

    /// Where the `[Default Applications]` section's lines start and end.
    fn defaults(lines: &[&str]) -> Option<(usize, usize)> {
        let start = lines.iter().position(|line| line.trim() == DEFAULTS)? + 1;
        let end = lines[start..]
            .iter()
            .position(|line| line.trim_start().starts_with('['))
            .map_or(lines.len(), |next| start + next);
        Some((start, end))
    }

    /// `scheme`'s line among the default applications, as it's written.
    fn default_line<'a>(text: &'a str, scheme: &str) -> Option<&'a str> {
        let lines: Vec<&str> = text.lines().collect();
        let (start, end) = defaults(&lines)?;
        lines[start..end]
            .iter()
            .find(|line| key(line) == Some(scheme))
            .copied()
    }

    /// A mimeapps.list with `scheme`'s default line set to `line` or taken
    /// out, its other sections and lines as they were.
    fn with_line(text: &str, scheme: &str, line: Option<&str>) -> String {
        let mut lines: Vec<&str> = text.lines().collect();
        match defaults(&lines) {
            None => {
                if let Some(line) = line {
                    if !lines.is_empty() {
                        lines.push("");
                    }
                    lines.extend([DEFAULTS, line]);
                }
            }
            Some((start, end)) => {
                let index = lines[start..end]
                    .iter()
                    .position(|line| key(line) == Some(scheme))
                    .map(|index| start + index);
                match (index, line) {
                    (Some(index), Some(line)) => lines[index] = line,
                    (Some(index), None) => {
                        lines.remove(index);
                    }
                    (None, Some(line)) => {
                        // After the section's last line, before a blank one.
                        let after = lines[start..end]
                            .iter()
                            .rposition(|line| !line.trim().is_empty())
                            .map_or(start, |last| start + last + 1);
                        lines.insert(after, line);
                    }
                    (None, None) => {}
                }
            }
        }
        let mut text = lines.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        text
    }

    /// Writes the list where it is: through a link, as home-manager or a
    /// dotfiles folder links it from elsewhere, which a new file would cut
    /// off, and keeping its permissions. One that can't be written there
    /// is reported rather than replaced.
    fn write_list(list: &Path, text: &str) -> io::Result<()> {
        let target = match std::fs::symlink_metadata(list) {
            Ok(metadata) if metadata.file_type().is_symlink() => std::fs::canonicalize(list)?,
            _ => list.to_owned(),
        };
        let permissions = match std::fs::metadata(&target) {
            Ok(metadata) => {
                // A rename would replace a file that can't be written.
                std::fs::OpenOptions::new().write(true).open(&target)?;
                metadata.permissions()
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::os::unix::fs::PermissionsExt::from_mode(0o644)
            }
            Err(error) => return Err(error),
        };
        let folder = target.parent().expect("the list is in a folder");
        std::fs::create_dir_all(folder)?;
        let mut file = tempfile::NamedTempFile::new_in(folder)?;
        file.write_all(text.as_bytes())?;
        file.as_file().set_permissions(permissions)?;
        file.persist(&target).map(drop).map_err(|error| error.error)
    }

    fn desktop_exists() -> bool {
        application_folders()
            .into_iter()
            .any(|folder| folder.join("applications").join(desktop()).exists())
    }

    fn application_folders() -> Vec<PathBuf> {
        let mut folders = Vec::new();
        if let Some(data) = desktop_entry::xdg_home("XDG_DATA_HOME", ".local/share") {
            folders.push(data);
        }
        let dirs = std::env::var_os("XDG_DATA_DIRS")
            .filter(|dirs| !dirs.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        folders.extend(std::env::split_paths(&dirs).filter(|path| path.is_absolute()));
        folders
    }

    /// The person's own default applications.
    fn list() -> Option<PathBuf> {
        desktop_entry::xdg_home("XDG_CONFIG_HOME", ".config")
            .map(|config| config.join("mimeapps.list"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const DISCORD: &str = SCHEMES[0];

        struct Folders {
            _folder: tempfile::TempDir,
            list: PathBuf,
            previous: PathBuf,
        }

        fn folders() -> Folders {
            let folder = tempfile::tempdir().unwrap();
            Folders {
                list: folder.path().join("config/mimeapps.list"),
                previous: folder.path().join(PREVIOUS),
                _folder: folder,
            }
        }

        fn read(path: &Path) -> String {
            std::fs::read_to_string(path).unwrap()
        }

        #[test]
        fn claims_and_gives_back_the_lines_as_they_were() {
            let folders = folders();
            let current = "[Added Associations]\n\
                x-scheme-handler/discord=other.desktop;\n\
                \n\
                [Default Applications]\n\
                text/html=browser.desktop;\n\
                x-scheme-handler/discord = discord.desktop;vesktop.desktop;\n\
                \n\
                [Removed Associations]\n";
            desktop_entry::write(&folders.list, current.as_bytes()).unwrap();
            assert!(!enabled_in(&folders.list));
            apply(&folders.list, &folders.previous, true).unwrap();
            assert!(enabled_in(&folders.list));
            let claimed = read(&folders.list);
            let ours = format!("x-scheme-handler/discord={};", desktop());
            assert!(claimed.contains(&format!("text/html=browser.desktop;\n{ours}\n")));
            assert!(
                claimed.contains("[Added Associations]\nx-scheme-handler/discord=other.desktop;")
            );
            assert!(claimed.contains(&format!(
                "x-scheme-handler/harmony={};\n\n[Removed",
                desktop()
            )));
            // A second claim keeps what the first kept.
            apply(&folders.list, &folders.previous, true).unwrap();
            apply(&folders.list, &folders.previous, false).unwrap();
            assert_eq!(read(&folders.list), current);
            assert!(!enabled_in(&folders.list));
            assert!(!folders.previous.exists());
        }

        #[test]
        fn takes_out_a_line_there_was_none_of() {
            let folders = folders();
            apply(&folders.list, &folders.previous, true).unwrap();
            assert_eq!(
                read(&folders.list),
                format!(
                    "[Default Applications]\nx-scheme-handler/discord={0};\nx-scheme-handler/harmony={0};\n",
                    desktop()
                )
            );
            apply(&folders.list, &folders.previous, false).unwrap();
            assert_eq!(read(&folders.list), "[Default Applications]\n");
            assert!(!enabled_in(&folders.list));
        }

        #[test]
        fn leaves_a_default_chosen_since() {
            let folders = folders();
            apply(&folders.list, &folders.previous, true).unwrap();
            let chosen = with_line(
                &read(&folders.list),
                DISCORD,
                Some("x-scheme-handler/discord=vesktop.desktop;"),
            );
            desktop_entry::write(&folders.list, chosen.as_bytes()).unwrap();
            apply(&folders.list, &folders.previous, false).unwrap();
            assert_eq!(
                default_line(&read(&folders.list), DISCORD),
                Some("x-scheme-handler/discord=vesktop.desktop;")
            );
        }

        #[test]
        fn writes_through_a_linked_list_and_never_replaces_one() {
            use std::os::unix::fs::PermissionsExt;
            let folders = folders();
            let dotfiles = folders.previous.with_file_name("dotfiles");
            std::fs::create_dir_all(&dotfiles).unwrap();
            std::fs::create_dir_all(folders.list.parent().unwrap()).unwrap();
            let target = dotfiles.join("mimeapps.list");
            std::fs::write(&target, "[Default Applications]\n").unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::os::unix::fs::symlink(&target, &folders.list).unwrap();
            apply(&folders.list, &folders.previous, true).unwrap();
            assert!(
                std::fs::symlink_metadata(&folders.list)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert!(enabled_in(&target));
            assert_eq!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );

            // As in a read-only store, the setting fails and the list stays.
            let before = read(&target);
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o400)).unwrap();
            assert!(apply(&folders.list, &folders.previous, false).is_err());
            assert_eq!(read(&target), before);
            assert!(
                std::fs::symlink_metadata(&folders.list)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
    }
}
