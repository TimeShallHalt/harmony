using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;
using Microsoft.Win32;

namespace Harmony.Setup
{
    internal sealed class Copy
    {
        internal string Directory, Exe, Engine, Uninstaller, Stamp, Product;
        internal Version Rank;
        internal bool Machine;
        internal string Label { get { return Rank.ToString(3) + " build " + Rank.Revision; } }
    }

    internal static class Inventory
    {
        internal static readonly StringComparer Names = StringComparer.OrdinalIgnoreCase;
        internal static readonly string Local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        internal static readonly string ProgramHome = Config.Fixture.Length == 0
            ? Path.Combine(Local, @"Programs\Harmony") : Path.Combine(Config.Fixture, "legacy");
        internal static readonly string Key = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\{" + Config.Id + "}_is1";
        internal static readonly Version Bundle = new Version(Config.Rank);
        internal static bool Equal(string a, string b) { return Names.Equals(a, b); }
        internal static bool Within(string path, string parent)
        {
            return path.StartsWith(parent.TrimEnd('\\') + "\\", StringComparison.OrdinalIgnoreCase);
        }
        internal static void Require(bool ok, string message)
        {
            if (!ok) throw new InvalidOperationException(message);
        }
        internal static string Full(string value)
        {
            Require(!String.IsNullOrWhiteSpace(value) && Regex.IsMatch(value, @"^[A-Za-z]:\\") &&
                value.IndexOfAny(new[] { '"', '\r', '\n', '\0' }) < 0, "Invalid local program path.");
            string path = Path.GetFullPath(value).TrimEnd('\\');
            Require(path.Length > 3 && Equal(path, value.TrimEnd('\\')), "Unexpected program path: " + value);
            return path;
        }
        // Treat junctions, symlinks and other reparse points as a boundary, including ancestors.
        internal static void CheckPath(string path)
        {
            for (string part = Full(path); part != null; part = Path.GetDirectoryName(part))
            {
                try { Require((File.GetAttributes(part) & FileAttributes.ReparsePoint) == 0, "Linked path needs attention: " + part); }
                catch (FileNotFoundException) { }
                catch (DirectoryNotFoundException) { }
            }
        }
        internal static string ProgramPath(string raw)
        {
            string dir = Full(raw);
            if (Config.Fixture.Length != 0)
                Require(Within(dir, Config.Fixture), "Outside the private test directory.");
            foreach (string reserved in new[] { Environment.GetFolderPath(Environment.SpecialFolder.Windows),
                Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), Path.Combine(Local, "Harmony") })
                Require(!Equal(dir, reserved) && !Within(dir, reserved), "This is a data or system folder: " + dir);
            CheckPath(dir);
            return dir;
        }
        internal static string Hash(string path)
        {
            using (var stream = File.Open(path, FileMode.Open, FileAccess.Read, FileShare.Read | FileShare.Delete))
            using (var sha = SHA256.Create())
                return BitConverter.ToString(sha.ComputeHash(stream)).Replace("-", "").ToLowerInvariant();
        }
        internal static string Value(RegistryKey key, string name)
        {
            return key.GetValue(name, "", RegistryValueOptions.DoNotExpandEnvironmentNames) as string ?? "";
        }
        internal static RegistryKey Hive(bool machine)
        {
            return RegistryKey.OpenBaseKey(machine ? RegistryHive.LocalMachine : RegistryHive.CurrentUser, RegistryView.Registry64);
        }
        internal static Copy FromBuild(string dir)
        {
            string exe = Path.Combine(dir, "Harmony.exe"), build = Path.Combine(dir, "BUILD.json");
            CheckPath(exe); CheckPath(build);
            Require(File.Exists(exe) && File.Exists(build), "Missing Harmony program or BUILD.json in " + dir);
            Require(new FileInfo(build).Length < 65536, "Unexpected Harmony build record in " + dir);
            var info = new JavaScriptSerializer().Deserialize<Dictionary<string, object>>(File.ReadAllText(build));
            string version = Convert.ToString(info["version"], CultureInfo.InvariantCulture);
            int number;
            Require(Regex.IsMatch(version, @"^\d+\.\d+\.\d+$") &&
                int.TryParse(Convert.ToString(info["build_id"], CultureInfo.InvariantCulture), out number),
                "Invalid Harmony version in " + dir);
            number = Convert.ToInt32(info["build_id"], CultureInfo.InvariantCulture);
            string hash = Convert.ToString(info["executable_sha256"]);
            var pe = FileVersionInfo.GetVersionInfo(exe);
            Require(number > 0 && Equal(pe.ProductName, "Harmony") && Equal(pe.OriginalFilename, "Harmony.exe") &&
                pe.ProductVersion == version && Regex.IsMatch(hash, @"^[a-fA-F0-9]{64}$") && Equal(Hash(exe), hash),
                "Harmony's program does not match its build record in " + dir);
            return new Copy { Directory = dir, Exe = exe, Rank = new Version(version + "." + number), Engine = "portable" };
        }
        static void FindInno(List<Copy> copies, bool machine)
        {
            using (var hive = Hive(machine))
            using (var key = hive.OpenSubKey(Key))
            {
                if (key == null) return;
                string dir = ProgramPath(Value(key, "InstallLocation")), command = Value(key, "UninstallString");
                Require(Value(key, "HarmonySimpleInstaller") == "1" && Equal(Value(key, "Publisher"), "Harmony contributors") &&
                    Regex.IsMatch(command, "^\"[^\"]+\"$"), "Unrecognized Harmony installer at " + dir);
                string unins = Full(command.Substring(1, command.Length - 2)), stamp = Value(key, "HarmonyUninstallerSHA256");
                CheckPath(unins);
                Require(Equal(Path.GetDirectoryName(unins), dir) && Regex.IsMatch(Path.GetFileName(unins), @"^unins\d{3}\.exe$", RegexOptions.IgnoreCase) &&
                    File.Exists(unins) && Equal(Hash(unins), stamp), "Cannot verify the registered Harmony uninstaller in " + dir);
                var copy = FromBuild(dir);
                Require(copy.Rank.ToString(3) == Value(key, "DisplayVersion"), "Harmony registration and program disagree in " + dir);
                copy.Engine = "inno"; copy.Machine = machine; copy.Uninstaller = unins; copy.Stamp = stamp;
                copies.Add(copy);
            }
        }
        [DllImport("msi.dll", CharSet = CharSet.Unicode)]
        static extern uint MsiEnumRelatedProducts(string upgrade, uint reserved, uint index, StringBuilder product);
        [DllImport("msi.dll", CharSet = CharSet.Unicode)]
        static extern uint MsiEnumProductsEx(string product, string sid, uint contexts, uint index, StringBuilder found, out uint context, StringBuilder owner, ref uint size);
        [DllImport("msi.dll", CharSet = CharSet.Unicode)]
        static extern uint MsiGetProductInfoEx(string product, string sid, uint context, string name, StringBuilder value, ref uint size);
        static string MsiValue(string product, string sid, uint context, string name)
        {
            var value = new StringBuilder(32768); uint size = (uint)value.Capacity;
            Require(MsiGetProductInfoEx(product, sid, context, name, value, ref size) == 0, "Cannot verify Harmony MSI " + name + ".");
            return value.ToString();
        }
        static void FindMsi(List<Copy> copies)
        {
            if (Config.Fixture.Length != 0) return;
            for (uint idx = 0; ; idx++)
            {
                var id = new StringBuilder(39);
                uint result = MsiEnumRelatedProducts(Config.MsiUpgrade, 0, idx, id);
                if (result == 259) break;
                Require(result == 0, "Windows Installer could not enumerate Harmony.");
                string product = id.ToString();
                for (uint n = 0; ; n++)
                {
                    var owner = new StringBuilder(256); uint size = (uint)owner.Capacity, context;
                    result = MsiEnumProductsEx(product, null, 7, n, new StringBuilder(39), out context, owner, ref size);
                    if (result == 259) break;
                    Require(result == 0 && (context == 2 || context == 4), "This Harmony MSI is managed by an administrator.");
                    bool machine = context == 4;
                    string sid = machine ? null : owner.ToString();
                    Func<string,string> get = name => MsiValue(product, sid, context, name);
                    Require(get("AssignmentType") == (machine ? "1" : "0") && get("Transforms") == "", "Unsupported Harmony MSI configuration.");
                    string dir = ProgramPath(get("InstallLocation"));
                    var ver = new Version(get("VersionString"));
                    var rank = new Version(ver.Major, ver.Minor, ver.Build / 1000, ver.Build % 1000);
                    Copy copy;
                    if (rank < new Version(1, 1, 0, 1))
                    {
                        Require(Equal(product, "{D58C04FC-2E46-5C5C-944B-6D62E7412A5E}") &&
                            Equal(Hash(get("LocalPackage")), "01d692e008871e4ff2859ec9df0acd0b52735404ae5f0556be25d11337d2196a"),
                            "Use Windows Apps to remove this earlier Harmony MSI before continuing.");
                        string exe = Path.Combine(dir, "yapfast.exe"); CheckPath(exe);
                        Require(File.Exists(exe), "Missing registered Harmony MSI program in " + dir);
                        copy = new Copy { Directory = dir, Exe = exe, Rank = rank };
                    }
                    else
                    {
                        copy = FromBuild(dir);
                        Require(copy.Rank == rank, "Harmony MSI and installed version disagree in " + dir);
                    }
                    copy.Engine = "msi"; copy.Product = product; copy.Machine = machine;
                    copies.Add(copy);
                }
            }
        }
        internal static List<Copy> Read()
        {
            var copies = new List<Copy>();
            FindInno(copies, true); FindInno(copies, false); FindMsi(copies);
            if (!copies.Any(c => Equal(c.Directory, ProgramHome)) && File.Exists(Path.Combine(ProgramHome, "Harmony.exe")) &&
                File.Exists(Path.Combine(ProgramHome, "BUILD.json")))
                copies.Add(FromBuild(ProgramPath(ProgramHome)));
            string releases = Path.Combine(ProgramHome, "releases"); CheckPath(releases);
            if (Directory.Exists(releases))
                foreach (string dir in Directory.GetDirectories(releases))
                {
                    if (!Regex.IsMatch(Path.GetFileName(dir), @"^\d+\.\d+\.\d+-b\d+$", RegexOptions.IgnoreCase)) continue;
                    CheckPath(dir);
                    if (!File.Exists(Path.Combine(dir, "Harmony.exe"))) continue;
                    var copy = FromBuild(ProgramPath(dir));
                    Require(Equal(Path.GetFileName(dir), copy.Rank.ToString(3) + "-b" + copy.Rank.Revision),
                        "Harmony's release directory and build version disagree: " + dir);
                    if (!copies.Any(c => Equal(c.Directory, dir))) copies.Add(copy);
                }
            Require(!copies.GroupBy(c => c.Directory, Names).Any(g => g.Count() > 1),
                "Two Windows installer records claim the same program directory. Removing either could remove the shared files.");
            return copies.OrderByDescending(c => c.Rank).ThenByDescending(c => c.Engine != "portable")
                .ThenByDescending(c => c.Machine).ThenBy(c => c.Directory, Names).ToList();
        }
    }
}
