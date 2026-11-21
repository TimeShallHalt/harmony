using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;

namespace Harmony.Setup
{
    // Use the wide shell interface so paths and profile arguments survive on every system code page.
    [ComImport, Guid("00021401-0000-0000-C000-000000000046")]
    internal class ShellLinkObject { }

    [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("000214F9-0000-0000-C000-000000000046")]
    internal interface IShellLinkW
    {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int size, IntPtr find, uint flags);
        void GetIDList(out IntPtr list);
        void SetIDList(IntPtr list);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder description, int size);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string description);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder directory, int size);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string directory);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder args, int size);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string args);
        void GetHotkey(out short hotkey);
        void SetHotkey(short hotkey);
        void GetShowCmd(out int command);
        void SetShowCmd(int command);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder icon, int size, out int index);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string icon, int index);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string path, uint reserved);
        void Resolve(IntPtr window, uint flags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string path);
    }

    public sealed class ShellLink
    {
        readonly IShellLinkW link;
        readonly string path;
        public ShellLink(string path)
        {
            this.path = path;
            link = (IShellLinkW)new ShellLinkObject();
            if (File.Exists(path)) ((IPersistFile)link).Load(path, 0);
        }
        public string TargetPath
        {
            get { var s = new StringBuilder(32768); link.GetPath(s, s.Capacity, IntPtr.Zero, 4); return Environment.ExpandEnvironmentVariables(s.ToString()); }
            set { link.SetPath(value); }
        }
        public string Arguments
        {
            get { var s = new StringBuilder(32768); link.GetArguments(s, s.Capacity); return s.ToString(); }
            set { link.SetArguments(value); }
        }
        public string WorkingDirectory
        {
            get { var s = new StringBuilder(32768); link.GetWorkingDirectory(s, s.Capacity); return s.ToString(); }
            set { link.SetWorkingDirectory(value); }
        }
        public void RetargetIcon(string old, string replacement)
        {
            var icon = new StringBuilder(32768); int index;
            link.GetIconLocation(icon, icon.Capacity, out index);
            if (string.Equals(icon.ToString(), old, StringComparison.OrdinalIgnoreCase)) link.SetIconLocation(replacement, index);
        }
        public void Save() { ((IPersistFile)link).Save(path, true); }
    }
}
