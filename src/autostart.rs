//! Starting Harmony when the person signs in to their computer: a value in the
//! registry's Run key on Windows, an XDG autostart entry on Linux. The entry is
//! the setting, so one turned off in Task Manager, or a deleted file, shows as
//! off in Settings too. It starts Harmony with `--background`, in the tray.

use std::path::PathBuf;

/// Whether this platform can start Harmony at sign-in.
pub const SUPPORTED: bool = cfg!(any(target_os = "linux", windows));

/// The name the entry goes by: the Run key's value, and the Inno Setup
/// installer's, which writes the same one.
#[cfg(windows)]
const NAME: &str = "Harmony";

/// Whether Harmony starts when the person signs in.
pub fn enabled() -> bool {
    platform::enabled()
}

/// Starts Harmony at sign-in, or stops it.
pub fn set(on: bool) -> std::io::Result<()> {
    match on {
        true => platform::write(&command()?),
        false => platform::remove(),
    }
}

/// Points an entry at this Harmony, after its folder moved or an update.
pub fn refresh() {
    #[cfg(target_os = "linux")]
    if let Err(error) = linux::refresh() {
        tracing::warn!(%error, "could not update starting at sign-in");
    }
    #[cfg(not(target_os = "linux"))]
    if enabled()
        && let Err(error) = set(true)
    {
        tracing::warn!(%error, "could not update starting at sign-in");
    }
}

/// What starts Harmony in the tray, with the data folder this one uses.
fn command() -> std::io::Result<Vec<String>> {
    let program = std::env::current_exe()?;
    #[cfg(target_os = "linux")]
    let program = crate::desktop_entry::appimage(&program).unwrap_or(program);
    let mut command = vec![path_text(program)?, "--background".to_owned()];
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(index) = arguments
        .iter()
        .position(|argument| argument == "--data-dir")
        && let Some(folder) = arguments.get(index + 1)
    {
        let folder = std::path::absolute(folder)?;
        command.extend(["--data-dir".to_owned(), path_text(folder)?]);
    }
    Ok(command)
}

pub(crate) fn path_text(path: PathBuf) -> std::io::Result<String> {
    path.into_os_string()
        .into_string()
        .map_err(|path| std::io::Error::other(format!("{} isn't valid Unicode", path.display())))
}

/// One argument of the command line, quoted when it holds a space, as
/// Windows splits a command line into arguments.
#[cfg(any(windows, test))]
pub(crate) fn windows_argument(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        return argument.to_owned();
    }
    windows_quoted(argument)
}

/// The program a command line runs, always quoted as Windows registers one:
/// a protocol handler's, and the `Run` key's, name it that way whatever the
/// path holds, so a path that later gains a space still runs that program
/// rather than a longer name of its own.
#[cfg(any(windows, test))]
pub(crate) fn windows_program(program: &str) -> String {
    windows_quoted(program)
}

/// `argument` quoted and escaped as Windows splits a command line.
#[cfg(any(windows, test))]
fn windows_quoted(argument: &str) -> String {
    // Backslashes count only before a quote, where each is doubled.
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(character);
                backslashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(not(any(target_os = "linux", windows)))]
use none as platform;
#[cfg(windows)]
use windows as platform;

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};

    use crate::desktop_entry;

    pub fn enabled() -> bool {
        super::command()
            .ok()
            .is_some_and(|command| config().is_some_and(|config| entry_enabled(&config, &command)))
    }

    pub fn write(command: &[String]) -> std::io::Result<()> {
        let config = config().ok_or_else(|| std::io::Error::other("no home folder"))?;
        let legacy = legacy_entry(&config);
        let legacy_owned = std::fs::read_to_string(&legacy)
            .is_ok_and(|text| desktop_entry::identity::owned(&text, command));
        // Keep an existing fork-owned legacy ID instead of inventing a global
        // alias or creating a second active autostart entry. An explicit setting
        // can enable an owned off entry; refresh below cannot.
        if !entry(&config).try_exists()? && legacy_owned {
            write_entry_at(&legacy, command)
        } else {
            write_entry(&config, command)
        }
    }

    pub fn remove() -> std::io::Result<()> {
        let Some(config) = config() else {
            return Ok(());
        };
        let command = super::command()?;
        for path in [entry(&config), legacy_entry(&config)] {
            if std::fs::read_to_string(&path)
                .is_ok_and(|text| desktop_entry::identity::owned(&text, &command))
            {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    pub fn refresh() -> std::io::Result<()> {
        let Some(config) = config() else {
            return Ok(());
        };
        let command = super::command()?;
        for path in [entry(&config), legacy_entry(&config)] {
            if let Ok(current) = std::fs::read_to_string(&path)
                && let Some(updated) = desktop_entry::identity::refreshed(&current, &command)
            {
                desktop_entry::write(&path, updated.as_bytes())?;
            }
        }
        Ok(())
    }

    /// `$XDG_CONFIG_HOME`, or `~/.config`.
    fn config() -> Option<PathBuf> {
        desktop_entry::xdg_home("XDG_CONFIG_HOME", ".config")
    }

    fn entry(config: &Path) -> PathBuf {
        config
            .join("autostart")
            .join(format!("{}.desktop", crate::icon::APP_ID))
    }

    fn legacy_entry(config: &Path) -> PathBuf {
        config
            .join("autostart")
            .join(format!("{}.desktop", desktop_entry::LEGACY_ID))
    }

    /// A legacy ID counts only with Harmony's name and exact launch context.
    pub(super) fn entry_enabled(config: &Path, command: &[String]) -> bool {
        [entry(config), legacy_entry(config)]
            .into_iter()
            .any(|path| {
                std::fs::read_to_string(path).is_ok_and(|text| {
                    desktop_entry::identity::owned(&text, command)
                        && desktop_entry::identity::active(&text)
                })
            })
    }

    pub(super) fn write_entry(config: &Path, command: &[String]) -> std::io::Result<()> {
        write_entry_at(&entry(config), command)
    }

    fn write_entry_at(path: &Path, command: &[String]) -> std::io::Result<()> {
        let exec = desktop_entry::identity::exec(command);
        let text = format!(
            "[Desktop Entry]\nType=Application\nName=Harmony\n\
             Comment=Starts Harmony in the system tray when you sign in\n\
             Exec={}\nIcon={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            exec,
            crate::icon::APP_ID,
        );
        desktop_entry::write(path, text.as_bytes())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn writes_an_entry_that_starts_in_the_tray() {
            let config = tempfile::tempdir().unwrap();
            let command = [
                "/opt/Harmony Builds/harmony".to_owned(),
                "--background".to_owned(),
                "--data-dir".to_owned(),
                "/home/a/100% $data".to_owned(),
            ];
            assert!(!entry_enabled(config.path(), &command));
            write_entry(config.path(), &command).unwrap();
            assert!(entry_enabled(config.path(), &command));
            let text = std::fs::read_to_string(entry(config.path())).unwrap();
            assert!(text.contains(
                r#"Exec="/opt/Harmony Builds/harmony" --background --data-dir "/home/a/100%% \$data""#
            ));
            assert!(text.contains("Name=Harmony\n"));
        }

        #[test]
        fn legacy_startup_requires_harmony_name_launch_context_and_active_state() {
            let config = tempfile::tempdir().unwrap();
            let command = [
                "/opt/Harmony/harmony",
                "--background",
                "--data-dir",
                "/home/a/live",
            ]
            .map(str::to_owned);
            let mut old = command.clone();
            old[0] = "/opt/Harmony/yapfast".to_owned();
            let path = legacy_entry(config.path());
            write_entry_at(&path, &old).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(entry_enabled(config.path(), &command));
            assert!(
                !entry(config.path()).exists(),
                "recognition must not create the new ID"
            );
            for changed in [
                text.replace("Name=Harmony", "Name=Yapfast"),
                text.replace("/home/a/live", "/home/a/isolated"),
                format!("{text}Hidden=true\n"),
                text.replace("enabled=true", "enabled=false"),
            ] {
                std::fs::write(&path, changed).unwrap();
                assert!(!entry_enabled(config.path(), &command));
            }
        }

        #[test]
        fn an_entry_a_desktop_turned_off_is_off() {
            let config = tempfile::tempdir().unwrap();
            let command = ["/opt/Harmony/harmony".to_owned(), "--background".to_owned()];
            write_entry(config.path(), &command).unwrap();
            let path = entry(config.path());
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(&path, text.replace("enabled=true", "enabled=false")).unwrap();
            assert!(!entry_enabled(config.path(), &command));
            std::fs::write(&path, "[Desktop Entry]\nHidden=true\n").unwrap();
            assert!(!entry_enabled(config.path(), &command));
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::io;

    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
        System::Registry::{
            HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW,
            RegGetValueW, RegSetKeyValueW,
        },
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Where Task Manager's Startup apps page keeps an entry turned off.
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    pub fn enabled() -> bool {
        let run = query(RUN, RRF_RT_REG_SZ);
        // Its first byte is odd while Task Manager has the entry turned off.
        let off = query(APPROVED, RRF_RT_REG_BINARY)
            .is_some_and(|approved| approved.first().is_some_and(|byte| byte & 1 == 1));
        run.is_some() && !off
    }

    pub fn write(command: &[String]) -> io::Result<()> {
        let line: Vec<String> = command
            .iter()
            .enumerate()
            .map(|(at, argument)| match at {
                0 => super::windows_program(argument),
                _ => super::windows_argument(argument),
            })
            .collect();
        let value = wide(&line.join(" "));
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                wide(RUN).as_ptr(),
                wide(super::NAME).as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        };
        checked(status)?;
        // Turning it on here turns it on in Task Manager too.
        delete(APPROVED)
    }

    pub fn remove() -> io::Result<()> {
        delete(RUN)?;
        delete(APPROVED)
    }

    fn delete(key: &str) -> io::Result<()> {
        let status = unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                wide(key).as_ptr(),
                wide(super::NAME).as_ptr(),
            )
        };
        match status {
            ERROR_FILE_NOT_FOUND => Ok(()),
            status => checked(status),
        }
    }

    /// The entry's value under `key`, if it has one of the `kind` asked for.
    fn query(key: &str, kind: u32) -> Option<Vec<u8>> {
        let (key, name) = (wide(key), wide(super::NAME));
        let mut size = 0u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                kind,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                kind,
                std::ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        (status == ERROR_SUCCESS).then(|| {
            data.truncate(size as usize);
            data
        })
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

    /// Writes the current user's registry: run it under Wine with a
    /// throwaway WINEPREFIX, not on a Windows account.
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        #[ignore = "writes the current user's Run key"]
        fn turns_on_and_off_as_task_manager_sees_it() {
            remove().unwrap();
            assert!(!enabled());
            write(&[
                r"C:\Program Files\Harmony\Harmony.exe".into(),
                "--background".into(),
            ])
            .unwrap();
            assert!(enabled());
            let value = query(RUN, RRF_RT_REG_SZ).unwrap();
            let text: Vec<u16> = value
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .take_while(|&unit| unit != 0)
                .collect();
            assert_eq!(
                String::from_utf16(&text).unwrap(),
                r#""C:\Program Files\Harmony\Harmony.exe" --background"#
            );
            // Task Manager turns it off with an odd first byte.
            let disabled = [3u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let status = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    wide(APPROVED).as_ptr(),
                    wide(super::super::NAME).as_ptr(),
                    windows_sys::Win32::System::Registry::REG_BINARY,
                    disabled.as_ptr().cast(),
                    disabled.len() as u32,
                )
            };
            checked(status).unwrap();
            assert!(!enabled());
            write(&["Harmony.exe".into()]).unwrap();
            assert!(enabled(), "turning it on here turns it on in Task Manager");
            remove().unwrap();
            assert!(!enabled());
            assert!(query(APPROVED, RRF_RT_REG_BINARY).is_none());
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod none {
    pub fn enabled() -> bool {
        false
    }

    pub fn write(_: &[String]) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "starting at sign-in isn't supported here",
        ))
    }

    pub fn remove() -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{windows_argument, windows_program};

    #[test]
    fn quotes_windows_arguments_as_windows_splits_them() {
        assert_eq!(windows_argument("--background"), "--background");
        assert_eq!(
            windows_argument(r"C:\Users\A B\Harmony\Harmony.exe"),
            r#""C:\Users\A B\Harmony\Harmony.exe""#
        );
        assert_eq!(windows_argument(r"C:\A B\"), r#""C:\A B\\""#);
        assert_eq!(windows_argument(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(windows_argument(""), r#""""#);
    }

    #[test]
    fn quotes_a_program_whether_or_not_it_needs_it() {
        assert_eq!(
            windows_program(r"C:\Apps\Harmony\Harmony.exe"),
            r#""C:\Apps\Harmony\Harmony.exe""#
        );
        assert_eq!(
            windows_program(r"C:\A B\Harmony.exe"),
            r#""C:\A B\Harmony.exe""#
        );
    }
}
