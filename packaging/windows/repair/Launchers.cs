using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text.RegularExpressions;
using Microsoft.Win32;

namespace Harmony.Setup
{
    internal sealed class Edit
    {
        internal string Description;
        internal Action Apply;
    }
    internal sealed class Launchers
    {
        readonly List<Edit> edits = new List<Edit>();
        readonly Copy keeper;
        readonly HashSet<string> known;
        readonly string userStart, sharedStart;
        readonly string registryPrefix = Config.Fixture.Length == 0 ? "" : @"Software\Harmony\InstallerTests\" + Config.Id + "\\";
        static string Folder(Environment.SpecialFolder folder, string testName)
        {
            return Config.Fixture.Length == 0 ? Environment.GetFolderPath(folder) : Path.Combine(Config.Fixture, testName);
        }
        internal Launchers(Copy keep, IEnumerable<string> old)
        {
            keeper = keep;
            known = new HashSet<string>(old, Inventory.Names);
            known.Add(keeper.Exe);
            userStart = Folder(Environment.SpecialFolder.Programs, "shortcuts");
            sharedStart = Folder(Environment.SpecialFolder.CommonPrograms, "common-shortcuts");
        }
        static bool DefaultArgs(string arguments)
        {
            string data = Path.Combine(Inventory.Local, @"Harmony\data");
            return String.IsNullOrWhiteSpace(arguments) || Inventory.Equal(arguments, "--data-dir " + data) ||
                Inventory.Equal(arguments, "--data-dir \"" + data + "\"");
        }
        static string Target(string link) { return File.Exists(link) ? new ShellLink(link).TargetPath : ""; }
        void Add(string label, Action apply) { edits.Add(new Edit { Description = label, Apply = apply }); }
        void InspectLink(string path, bool sharedCanonical)
        {
            var link = new ShellLink(path);
            string before = link.TargetPath;
            if (!known.Contains(before)) return;
            // Shared shortcuts belong to the shared installation. Its uninstaller retires them
            // when moving to personal scope. Never point other users into a personal program folder.
            if (!keeper.Machine && !Inventory.Equal(before, keeper.Exe) &&
                (Inventory.Within(path, sharedStart) || Inventory.Within(path, Folder(Environment.SpecialFolder.CommonDesktopDirectory, "common-desktop")))) return;
            Inventory.CheckPath(path);
            string args = link.Arguments, work = link.WorkingDirectory;
            string name = Path.GetFileName(path);
            bool userStandard = Inventory.Equal(path, Path.Combine(userStart, "Harmony.lnk"));
            bool start = Inventory.Equal(Path.GetDirectoryName(path), userStart) || Inventory.Equal(Path.GetDirectoryName(path), sharedStart);
            bool preview = start && Inventory.Equal(name, "Harmony Preview.lnk") && Regex.IsMatch(args, @"(?:^|\s)--preview(?:\s|$)", RegexOptions.IgnoreCase);
            if (preview || (userStandard && sharedCanonical && DefaultArgs(args)))
            {
                Add("Remove duplicate shortcut " + path, delegate {
                    if (!File.Exists(path)) return;
                    Inventory.CheckPath(path);
                    var now = new ShellLink(path);
                    Inventory.Require(Inventory.Equal(now.TargetPath, before) && now.Arguments == args, "Shortcut changed: " + path);
                    File.Delete(path);
                });
            }
            else if (!Inventory.Equal(before, keeper.Exe))
            {
                Add("Retarget shortcut " + path, delegate {
                    Inventory.CheckPath(path);
                    var now = new ShellLink(path);
                    Inventory.Require(Inventory.Equal(now.TargetPath, before) && now.Arguments == args, "Shortcut changed: " + path);
                    now.TargetPath = keeper.Exe;
                    if (Inventory.Equal(work, Path.GetDirectoryName(before))) now.WorkingDirectory = keeper.Directory;
                    now.RetargetIcon(before, keeper.Exe);
                    now.Save();
                    var after = new ShellLink(path);
                    Inventory.Require(Inventory.Equal(after.TargetPath, keeper.Exe) && after.Arguments == args,
                        "Could not verify shortcut " + path);
                });
            }
        }
        // Replace only the executable (or icon path). Everything following it retains its exact spelling.
        static string Replaced(string text, string before, string after, bool icon)
        {
            if (String.IsNullOrEmpty(text)) return null;
            bool quoted = text[0] == '"';
            int end;
            if (quoted)
            {
                end = text.IndexOf('"', 1);
                if (end < 1 || !Inventory.Equal(Environment.ExpandEnvironmentVariables(text.Substring(1, end - 1)), before)) return null;
                end++;
            }
            else
            {
                if (!text.StartsWith(before, StringComparison.OrdinalIgnoreCase)) return null;
                end = before.Length;
            }
            if (end < text.Length && !(icon ? text[end] == ',' : Char.IsWhiteSpace(text[end]))) return null;
            return (icon && !quoted ? after : "\"" + after + "\"") + text.Substring(end);
        }
        void RegistryValue(string keyName, string name, bool icon)
        {
            string path = registryPrefix + keyName;
            using (var hive = Inventory.Hive(false))
            using (var key = hive.OpenSubKey(path))
            {
                if (key == null) return;
                string before = Inventory.Value(key, name);
                if (before.Length == 0) return;
                var kind = key.GetValueKind(name);
                if (kind != RegistryValueKind.String && kind != RegistryValueKind.ExpandString) return;
                foreach (string old in known.Where(v => !Inventory.Equal(v, keeper.Exe)))
                {
                    string after = Replaced(before, old, keeper.Exe, icon);
                    if (after == null) continue;
                    Add("Retarget " + keyName + (name == "" ? "" : "\\" + name), delegate {
                        using (var root = Inventory.Hive(false))
                        using (var current = root.OpenSubKey(path, true))
                        {
                            Inventory.Require(current != null && Inventory.Value(current, name) == before, "Windows launcher changed: " + keyName);
                            current.SetValue(name, after, kind);
                            Inventory.Require(Inventory.Value(current, name) == after, "Could not verify " + keyName);
                        }
                    });
                    break;
                }
            }
        }
        internal List<Edit> Plan()
        {
            string user = Path.Combine(userStart, "Harmony.lnk"), shared = Path.Combine(sharedStart, "Harmony.lnk");
            bool sharedCanonical = Inventory.Equal(Target(shared), keeper.Exe) && DefaultArgs(new ShellLink(shared).Arguments);
            var roots = new[] { userStart, sharedStart, Path.Combine(userStart,"Harmony"), Path.Combine(sharedStart,"Harmony"),
                Folder(Environment.SpecialFolder.DesktopDirectory,"desktop"), Folder(Environment.SpecialFolder.CommonDesktopDirectory,"common-desktop"),
                Folder(Environment.SpecialFolder.Startup,"startup"), Config.Fixture.Length == 0
                    ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), @"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar")
                    : Path.Combine(Config.Fixture,"pins") };
            foreach (string root in roots.Distinct(Inventory.Names))
            {
                if (String.IsNullOrEmpty(root) || !Directory.Exists(root)) continue;
                foreach (string file in Directory.GetFiles(root, "*.lnk")) InspectLink(file, sharedCanonical);
            }
            string standard = File.Exists(user) && !known.Contains(Target(user)) ? Path.Combine(userStart, @"Harmony\Harmony.lnk") : user;
            string sharedTarget = Target(shared);
            if (!known.Contains(Target(standard)) && !Inventory.Equal(sharedTarget, keeper.Exe) && !(keeper.Machine && known.Contains(sharedTarget)))
            {
                Inventory.Require(!File.Exists(standard), "An unrelated shortcut already occupies " + standard);
                Add("Create Start shortcut " + standard, delegate {
                    Inventory.CheckPath(standard);
                    Inventory.Require(!File.Exists(standard), "The Start shortcut changed.");
                    Directory.CreateDirectory(Path.GetDirectoryName(standard));
                    var link = new ShellLink(standard);
                    link.TargetPath = keeper.Exe; link.WorkingDirectory = keeper.Directory; link.Save();
                    Inventory.Require(Inventory.Equal(Target(standard), keeper.Exe), "Could not verify the new Start shortcut.");
                });
            }
            foreach (string scheme in new[] { "discord", "harmony", "yapfast" })
            {
                RegistryValue(@"Software\Classes\" + scheme + @"\shell\open\command", "", false);
                RegistryValue(@"Software\Classes\" + scheme + @"\DefaultIcon", "", true);
            }
            foreach (string name in new[] { "Harmony", "Yapfast" })
                RegistryValue(@"Software\Microsoft\Windows\CurrentVersion\Run", name, false);
            return edits;
        }
        [DllImport("shell32.dll")]
        internal static extern void SHChangeNotify(int change, uint flags, IntPtr item1, IntPtr item2);
    }
}
