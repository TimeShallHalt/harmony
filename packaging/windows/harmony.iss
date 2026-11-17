; Harmony's optional Windows Inno installer. scripts/package-windows.py compiles
; this offline over the same files as the portable ZIP. The primary fork installer
; is scripts/package_harmony_msi.py; neither provides an automatic update service.
; Based on Yapfast's packaging; original application credits/licenses are retained.

#ifndef AppVersion
  #error Compile through scripts/package-windows.py, which defines AppVersion, VersionNumber and SourceDir
#endif
#ifdef Edition
  #define DisplayName "Harmony (" + Edition + ")"
#else
  #define DisplayName "Harmony"
  #define FileSuffix ""
#endif

[Setup]
; Stable fork-specific ID, distinct from upstream Yapfast and from the MSI product.
AppId={{46969CAC-C472-58C4-89BD-069E35950175}
AppName=Harmony
AppVersion={#AppVersion}
AppVerName={#DisplayName} {#AppVersion}
AppPublisher=Harmony contributors
VersionInfoVersion={#VersionNumber}
VersionInfoProductName=Harmony
VersionInfoDescription=Harmony offline installer
PrivilegesRequired=lowest
DefaultDirName={autopf}\Harmony
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputBaseFilename=Harmony-{#AppVersion}{#FileSuffix}-windows-x64-setup
SetupIconFile=harmony.ico
UninstallDisplayIcon={app}\Harmony.exe
UninstallDisplayName={#DisplayName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; Ask the user to quit. Never terminate or relaunch Harmony, or start it after setup.
CloseApplications=no
RestartApplications=no
#ifdef LicenseFile
LicenseFile={#LicenseFile}
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Harmony"; Filename: "{app}\Harmony.exe"
Name: "{autodesktop}\Harmony"; Filename: "{app}\Harmony.exe"; Tasks: desktopicon

; No startup registration, protocol claims, update URLs, [Run], [UninstallRun],
; settings cleanup, or automatic application/process actions.
