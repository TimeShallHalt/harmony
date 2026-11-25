using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Security.Principal;
using System.Text;
using System.Threading;

namespace Harmony.Setup
{
    internal static class Repair
    {
        static readonly Dictionary<string,string> report = new Dictionary<string,string>();
        static bool Elevated()
        {
            return new WindowsPrincipal(WindowsIdentity.GetCurrent()).IsInRole(WindowsBuiltInRole.Administrator);
        }
        static void CheckStopped(IEnumerable<Copy> affected)
        {
            var paths = new HashSet<string>(affected.Select(c => c.Exe), Inventory.Names);
            if (paths.Count == 0) return;
            foreach (string name in paths.Select(Path.GetFileNameWithoutExtension).Distinct(Inventory.Names))
                foreach (var process in Process.GetProcessesByName(name))
                    using (process)
                    {
                        string path;
                        try { path = process.MainModule.FileName; }
                        catch (InvalidOperationException) { continue; }
                        catch (System.ComponentModel.Win32Exception) {
                            if (process.HasExited) continue;
                            throw new IOException("Could not identify a running " + name + " process. Close that copy and try again.");
                        }
                        Inventory.Require(!paths.Contains(path), "Quit the older or affected Harmony process in " + Path.GetDirectoryName(path) + " and try again.");
                    }
        }
        static void RemovePortable(Copy copy)
        {
            string releases = Path.Combine(Inventory.ProgramHome, "releases");
            Inventory.Require(Inventory.Equal(Path.GetDirectoryName(copy.Directory), releases) ||
                Inventory.Equal(copy.Directory, Inventory.ProgramHome), "Unexpected release directory.");
            Inventory.CheckPath(copy.Directory);
            string name = copy.Rank.ToString(3) + "-b" + copy.Rank.Revision + ".sha256";
            var dirs = new HashSet<string>(Inventory.Names);
            using (Stream stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(name))
            {
                // Older uncatalogued releases can still retire their verified program. Preserve other contents.
                if (stream != null)
                    using (var reader = new StreamReader(stream, Encoding.UTF8))
                        for (string line; (line = reader.ReadLine()) != null;)
                        {
                            string relative = line.Substring(66).Replace('/', '\\');
                            if (Inventory.Equal(relative, "Harmony.exe") || Inventory.Equal(relative, "BUILD.json")) continue;
                            string path = Inventory.Full(Path.Combine(copy.Directory, relative));
                            Inventory.Require(Inventory.Within(path, copy.Directory), "Invalid release ownership record.");
                            for (string parent = Path.GetDirectoryName(path); Inventory.Within(parent, copy.Directory); parent = Path.GetDirectoryName(parent))
                                dirs.Add(parent);
                            if (!File.Exists(path)) continue;
                            Inventory.CheckPath(path);
                            if (Inventory.Equal(Inventory.Hash(path), line.Substring(0,64))) File.Delete(path);
                        }
            }
            foreach (string dir in dirs.OrderByDescending(s => s.Length)) RemoveIfEmpty(dir);
            // Keep the identity record available until the executable has actually been removed.
            var verified = Inventory.FromBuild(copy.Directory);
            Inventory.Require(verified.Rank == copy.Rank, "The old release changed during Repair.");
            File.Delete(copy.Exe);
            File.Delete(Path.Combine(copy.Directory, "BUILD.json"));
            RemoveIfEmpty(copy.Directory);
        }
        static void RemoveIfEmpty(string dir)
        {
            Inventory.CheckPath(dir);
            if (Directory.Exists(dir) && !Directory.EnumerateFileSystemEntries(dir).Any()) Directory.Delete(dir, false);
        }
        static void Retire(Copy copy)
        {
            CheckStopped(new[] {copy});
            Inventory.Require(copy.Machine || !Elevated(), "Start Setup normally from File Explorer to repair current-user programs.");
            if (copy.Engine == "portable") { RemovePortable(copy); return; }
            string file, args;
            if (copy.Engine == "msi")
            {
                file = Path.Combine(Environment.SystemDirectory, "msiexec.exe");
                args = "/x " + copy.Product + " /qn /norestart REBOOT=ReallySuppress MSIRESTARTMANAGERCONTROL=Disable MSIDISABLERMRESTART=1";
            }
            else
            {
                file = copy.Uninstaller; Inventory.CheckPath(file);
                Inventory.Require(Inventory.Equal(Inventory.Hash(file), copy.Stamp), "The Harmony uninstaller changed.");
                if (copy.Machine && !Elevated())
                    Inventory.Require(Inventory.Within(file, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles)),
                        "Run the verified shared uninstaller in its custom program folder before continuing.");
                args = "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART";
            }
            var start = new ProcessStartInfo(file, args) { UseShellExecute = true, WorkingDirectory = Environment.SystemDirectory };
            if (copy.Machine && !Elevated()) start.Verb = "runas";
            using (var proc = Process.Start(start))
            {
                proc.WaitForExit();
                Inventory.Require(proc.ExitCode == 0, "Windows could not remove " + copy.Directory + " (exit " + proc.ExitCode + ").");
            }
            for (int i = 0; i < 100 && File.Exists(copy.Exe); i++) Thread.Sleep(100);
            Inventory.Require(!File.Exists(copy.Exe), "The old program is still present at " + copy.Exe);
            Inventory.Require(!Inventory.Read().Any(c => Inventory.Equal(c.Directory, copy.Directory)), "The old program is still registered: " + copy.Directory);
        }
        static void Summarize(List<Copy> copies, bool machine)
        {
            report["count"] = copies.Count.ToString();
            var preferred = copies.FirstOrDefault(c => c.Engine != "portable" && c.Machine == machine)
                ?? copies.FirstOrDefault(c => c.Engine != "portable");
            if (preferred != null)
            {
                report["previous"] = preferred.Directory;
                report["sameScope"] = (preferred.Machine == machine ? "1" : "0");
            }
            if (copies.Count == 0) return;
            var keep = copies[0];
            report["keeper"] = keep.Directory; report["version"] = keep.Label;
            report["compare"] = Math.Sign(keep.Rank.CompareTo(Inventory.Bundle)).ToString();
            var edits = new Launchers(keep, copies.Select(c => c.Exe)).Plan();
            report["changes"] = (copies.Count - 1 + edits.Count).ToString();
            for (int i = 0; i < copies.Count; i++) report["copy" + i] = copies[i].Label + "  " + copies[i].Directory;
            for (int i = 0; i < edits.Count; i++) report["edit" + i] = edits[i].Description;
        }
        static void Clean(List<Copy> copies, Copy keep, IEnumerable<string> aliases)
        {
            Inventory.Require(copies.All(c => c.Rank <= keep.Rank), "A newer Harmony is installed. Run Repair again.");
            var old = copies.Where(c => c != keep).ToList();
            CheckStopped(old);
            var oldPaths = old.Select(c => c.Exe).Concat(aliases).Distinct(Inventory.Names).ToList();
            var changes = new Launchers(keep, oldPaths).Plan();
            // The original user's token owns the per-user integrations and program files.
            if (changes.Count > 0 || old.Any(c => !c.Machine))
                Inventory.Require(!Elevated(), "Start Harmony Setup normally from File Explorer to repair your account's launchers.");
            foreach (var edit in changes) edit.Apply();
            foreach (var copy in old) Retire(copy);
            // Older uninstallers can remove the Start shortcut they originally created.
            var final = new Launchers(keep, oldPaths).Plan();
            foreach (var edit in final) edit.Apply();
            if (changes.Count + final.Count > 0) Launchers.SHChangeNotify(0x08000000, 0, IntPtr.Zero, IntPtr.Zero);
            var remaining = Inventory.Read();
            Inventory.Require(remaining.Count == 1 && Inventory.Equal(remaining[0].Exe, keep.Exe), "Could not verify a single remaining Harmony installation.");
            Inventory.Require(new Launchers(keep, oldPaths).Plan().Count == 0, "Could not verify repaired launchers.");
            report["removed"] = old.Count.ToString(); report["launchers"] = (changes.Count + final.Count).ToString();
            report["keeper"] = keep.Directory; report["version"] = keep.Label;
        }
        static void Run(string[] args)
        {
            Inventory.Require(args.Length == 6, "Invalid Setup operation.");
            string mode = args[0], target = args[3], expected = args[4], plan = args[5];
            bool machine = args[2] == "all";
            var copies = Inventory.Read();
            if (mode == "scan") { Summarize(copies, machine); return; }
            if (mode == "repair")
            {
                Inventory.Require(copies.Count > 0, "No installed Harmony was found. Choose Install to add it.");
                Clean(copies, copies[0], new string[0]);
                return;
            }
            target = Inventory.ProgramPath(target);
            Inventory.Require(copies.All(c => c.Rank <= Inventory.Bundle), "A newer Harmony is already installed. Choose Repair to keep it.");
            if (mode == "prepare")
            {
                if (expected.Length != 0)
                    Inventory.Require(Inventory.Equal(target, expected) && copies.Any(c => c.Engine == "inno" &&
                        c.Machine == machine && Inventory.Equal(c.Directory, target)), "The installation selected by the updater has changed.");
                string destination = Path.Combine(target, "Harmony.exe");
                Inventory.Require(!File.Exists(destination) || copies.Any(c => Inventory.Equal(c.Exe, destination)),
                    "An unregistered executable already occupies the destination. Choose a different program folder.");
                Inventory.Require(!copies.Any(c => c.Engine == "portable" && Inventory.Equal(c.Directory, target) &&
                    !Inventory.Equal(c.Directory, Inventory.ProgramHome)),
                    "Choose an install folder outside the existing versioned release.");
                CheckStopped(copies);
                File.WriteAllLines(plan, copies.Select(c => c.Exe), Encoding.Unicode);
                // A conflicting Windows registration must be freed before Inno writes its new one.
                // Other copies survive until the replacement has been installed and verified.
                foreach (var c in copies.Where(c =>
                    (Inventory.Equal(c.Directory, target) && (c.Engine != "inno" || c.Machine != machine)) ||
                    (c.Engine == "inno" && c.Machine == machine && !Inventory.Equal(c.Directory, target)))) Retire(c);
                return;
            }
            Inventory.Require(mode == "finish", "Unknown Setup operation.");
            var keep = copies.FirstOrDefault(c => c.Engine == "inno" && c.Machine == machine && Inventory.Equal(c.Directory, target));
            Inventory.Require(keep != null && keep.Rank == Inventory.Bundle && Inventory.Equal(Inventory.Hash(keep.Exe), Config.Hash),
                "The new Harmony program could not be verified.");
            Clean(copies, keep, File.ReadAllLines(plan, Encoding.Unicode));
        }
        [STAThread]
        static int Main(string[] args)
        {
            // The wizard supplies its own temporary report path. Use UTF-16 for Inno's INI reader.
            if (args.Length < 2) return 2;
            int status = 0;
            try { Run(args); }
            catch (Exception e) { status = 1; report["error"] = e.GetBaseException().Message; }
            var lines = new List<string> { "[repair]", "status=" + status };
            lines.AddRange(report.Select(p => p.Key + "=" + p.Value.Replace('\r',' ').Replace('\n',' ')));
            File.WriteAllLines(args[1], lines, Encoding.Unicode);
            return status;
        }
    }
}
