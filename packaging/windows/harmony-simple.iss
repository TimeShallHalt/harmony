; A standard Inno Setup GUI. No custom coordinator, bridge, service or data migration.
#ifndef PayloadDir
  #error Use scripts/build-harmony-installer.py
#endif
#ifndef PayloadFiles
  #error Missing explicit payload file list
#endif
#ifndef AppVersion
  #error Missing Harmony application version
#endif
#ifndef HarmonyId
  #define HarmonyId "46969CAC-C472-58C4-89BD-069E35950175"
#endif
#ifdef FixtureRoot
  #if HarmonyId == "46969CAC-C472-58C4-89BD-069E35950175"
    #error Private fixtures require a different AppId
  #endif
#endif

[Setup]
AppId={{{#HarmonyId}}
AppName=Harmony
AppVersion={#AppVersion}
AppPublisher=Harmony contributors
VersionInfoVersion={#AppVersion}.{#AppBuild}
VersionInfoDescription=Harmony Installer
DefaultDirName={code:DefaultDestination}
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
UsePreviousPrivileges=no
UsePreviousAppDir=no
DisableDirPage=no
DisableProgramGroupPage=yes
DisableWelcomePage=no
SetupArchitecture=x64
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
SetupIconFile=harmony.ico
UninstallDisplayIcon={app}\Harmony.exe
LicenseFile={#PayloadDir}\LICENSE.txt
OutputBaseFilename=Harmony-{#AppVersion}-Setup
Compression=lzma2/normal
SolidCompression=yes
CloseApplications=no
RestartApplications=no
AlwaysRestart=no
RestartIfNeededByRun=no

[Messages]
PrivilegesRequiredOverrideTitle=Harmony installation
PrivilegesRequiredOverrideInstruction=Who should have access to Harmony?
PrivilegesRequiredOverrideText1=Choose all users to install or update the shared app, or me only to move it to your account. Saved data is kept.
PrivilegesRequiredOverrideText2=Choose me only for your account, or all users to install or update the shared app. Saved data is kept.
PrivilegesRequiredOverrideAllUsers=Install / update for &all users
PrivilegesRequiredOverrideAllUsersRecommended=Install / update for &all users
PrivilegesRequiredOverrideCurrentUser=Install / move to &me only
PrivilegesRequiredOverrideCurrentUserRecommended=Install / move to &me only

[Files]
#include PayloadFiles
; Ordinary logged file copy preserves old shared executable paths after relocation.
Source: "{#PayloadDir}\Harmony.exe"; DestDir: "{code:CompatibilityDirectory}"; DestName: "{code:CompatibilityName}"; Flags: ignoreversion; Check: NeedsCompatibilityCopy

[Icons]
Name: "{code:ProgramsDirectory}\Harmony"; Filename: "{app}\Harmony.exe"; WorkingDir: "{app}"; Check: MayCreateShortcut

[Code]
const
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{{#HarmonyId}}_is1';
  LegacyProduct = '{D58C04FC-2E46-5C5C-944B-6D62E7412A5E}';
  LegacyCacheHash = '01d692e008871e4ff2859ec9df0acd0b52735404ae5f0556be25d11337d2196a';
var
  OldDirectory, OldExecutable, OldUninstaller, OldVersion, OldKind, DetectionError: String;
  OldMachine, CompatibilityCopy, PreviousRemoved: Boolean;

function EnumMsi(Product: String; Sid: NativeInt; Context, Index: LongWord; Found: String;
  var FoundContext: LongWord; Owner: String; var OwnerLength: LongWord): LongWord;
  external 'MsiEnumProductsExW@msi.dll stdcall';
function MachineMsiInfo(Product: String; Sid: NativeInt; Context: LongWord;
  PropertyName: String; Value: String; var Size: LongWord): LongWord;
  external 'MsiGetProductInfoExW@msi.dll stdcall';
function UserMsiInfo(Product, Sid: String; Context: LongWord;
  PropertyName: String; Value: String; var Size: LongWord): LongWord;
  external 'MsiGetProductInfoExW@msi.dll stdcall';

function SamePath(A, B: String): Boolean;
begin
  Result := CompareText(RemoveBackslashUnlessRoot(A), RemoveBackslashUnlessRoot(B)) = 0;
end;
function UnderDirectory(Path, Parent: String): Boolean;
begin
  Parent := AddBackslash(Parent);
  Result := CompareText(Copy(Path, 1, Length(Parent)), Parent) = 0;
end;
function ProgramDirectory(Path: String): Boolean;
begin
  Result := (Length(Path) > 3) and (Copy(Path, 2, 2) = ':\') and
    (Pos('..', Path) = 0) and (Pos('"', Path) = 0) and
    not SamePath(Path, ExpandConstant('{win}')) and not UnderDirectory(Path, ExpandConstant('{win}')) and
    not SamePath(Path, ExpandConstant('{commonappdata}')) and not UnderDirectory(Path, ExpandConstant('{commonappdata}')) and
    not SamePath(Path, ExpandConstant('{localappdata}\Harmony')) and not UnderDirectory(Path, ExpandConstant('{localappdata}\Harmony'));
end;
procedure AddPrevious(Directory, ExeName, Uninstaller, Version, Kind: String; Machine: Boolean);
begin
  if OldDirectory <> '' then begin
    DetectionError := 'Multiple registered Harmony copies were found. Remove the extra program through Windows Apps first. Keep saved data.';
    Exit;
  end;
  if not ProgramDirectory(Directory) then begin DetectionError := 'Invalid previous program location. No files changed.'; Exit; end;
  OldDirectory := RemoveBackslashUnlessRoot(Directory); OldExecutable := ExeName;
  OldUninstaller := Uninstaller; OldVersion := Version; OldKind := Kind; OldMachine := Machine;
end;
procedure FindInno(Root: Integer; Machine: Boolean);
var Directory, Command, Marker, Stamp, Version, Exe: String;
begin
  if not RegKeyExists(Root, UninstallKey) then Exit;
  RegQueryStringValue(Root, UninstallKey, 'InstallLocation', Directory);
  RegQueryStringValue(Root, UninstallKey, 'UninstallString', Command);
  RegQueryStringValue(Root, UninstallKey, 'DisplayVersion', Version);
  RegQueryStringValue(Root, UninstallKey, 'HarmonySimpleInstaller', Marker);
  RegQueryStringValue(Root, UninstallKey, 'HarmonyUninstallerSHA256', Stamp);
  if (Marker <> '1') or (Length(Command) < 3) or (Copy(Command, 1, 1) <> '"') or
     (Copy(Command, Length(Command), 1) <> '"') then begin
    DetectionError := 'Unsupported previous installer. Uninstall its program through Windows Apps first; keep saved data.'; Exit;
  end;
  Exe := Copy(Command, 2, Length(Command) - 2);
  if (Pos('"', Exe) <> 0) or not SamePath(ExtractFileDir(Exe), Directory) or not FileExists(Exe) or (Stamp = '') then begin
    DetectionError := 'Previous uninstaller could not be verified. No files changed.'; Exit;
  end;
  if CompareText(GetSHA256OfFile(Exe), Stamp) <> 0 then begin DetectionError := 'Previous uninstaller has changed. No files changed.'; Exit; end;
  AddPrevious(Directory, 'Harmony.exe', Exe, Version, 'inno', Machine);
end;
function MsiValue(Sid: String; Context: LongWord; Name: String): String;
var Size, Code: LongWord;
begin
  Size := 32767; Result := StringOfChar(#0, Size + 1);
  if Context = 4 then Code := MachineMsiInfo(LegacyProduct, 0, Context, Name, Result, Size)
  else Code := UserMsiInfo(LegacyProduct, Sid, Context, Name, Result, Size);
  if Code <> 0 then RaiseException('Cannot read previous MSI (' + IntToStr(Code) + ').');
  SetLength(Result, Size);
end;
procedure FindLegacyMsi;
#ifndef FixtureRoot
var Product, Owner, Directory, Cache, Version, Assignment: String; Context, Size, Code, Index: LongWord;
#endif
begin
#ifndef FixtureRoot
  for Index := 0 to 1 do begin
    Product := StringOfChar(#0, 39); Owner := StringOfChar(#0, 256); Size := 255;
    Code := EnumMsi(LegacyProduct, 0, 7, Index, Product, Context, Owner, Size);
    if Code = 259 then Exit;
    if Code <> 0 then RaiseException('Cannot discover previous MSI (' + IntToStr(Code) + ').');
    SetLength(Owner, Size);
    if (Context <> 2) and (Context <> 4) then RaiseException('Managed installation: contact its administrator.');
    Version := MsiValue(Owner, Context, 'VersionString'); Assignment := MsiValue(Owner, Context, 'AssignmentType');
    if ((Context = 4) and (Assignment <> '1')) or ((Context = 2) and (Assignment <> '0')) then RaiseException('Previous MSI scope mismatch.');
    Directory := MsiValue(Owner, Context, 'InstallLocation'); Cache := MsiValue(Owner, Context, 'LocalPackage');
    if (Version <> '1.0.3001') or (MsiValue(Owner, Context, 'Transforms') <> '') or
       (CompareText(GetSHA256OfFile(Cache), LegacyCacheHash) <> 0) then
      RaiseException('Unsupported older MSI. Uninstall its program through Windows Apps first; keep saved data.');
    AddPrevious(Directory, 'yapfast.exe', '', Version, 'msi', Context = 4);
  end;
#endif
end;
function OpenParent(Access: LongWord; Inherit: Integer; Pid: LongWord): NativeUInt;
  external 'OpenProcess@kernel32.dll stdcall';
function WaitForParent(Handle: NativeUInt; Milliseconds: LongWord): LongWord;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseParent(Handle: NativeUInt): Integer;
  external 'CloseHandle@kernel32.dll stdcall';

procedure WaitForHarmony;
var Id: Integer; Handle: NativeUInt; Param: String;
begin
  Param := ExpandConstant('{param:HARMONYWAITPID|}');
  if Param = '' then Exit;
  Id := StrToIntDef(Param, 0);
  if Id <= 0 then RaiseException('Invalid Harmony process ID.');
  Handle := OpenParent($00100000, 0, Id);
  if Handle = 0 then Exit; { The application can exit before Setup starts. }
  Log('Waiting for Harmony to quit before updating.');
  try
    if WaitForParent(Handle, 60000) <> 0 then RaiseException('Harmony is still running. Quit it and try again.');
  finally CloseParent(Handle); end;
end;

function InitializeSetup: Boolean;
var Expected: String;
begin
  Result := False;
  try
    WaitForHarmony;
    FindInno(HKLM64, True); FindInno(HKCU, False); FindLegacyMsi;
    Expected := ExpandConstant('{param:HARMONYINSTALLED|}');
    if (Expected <> '') and ((DetectionError <> '') or (OldKind <> 'inno') or
        (OldMachine <> IsAdminInstallMode) or not SamePath(Expected, OldDirectory)) then
      RaiseException('The Harmony installation changed. Open Setup to choose the installation to update.');
    Log('Harmony previous install: ' + OldKind + ' ' + OldVersion + ' at ' + OldDirectory);
    Result := True;
  except SuppressibleMsgBox(GetExceptionMessage, mbError, MB_OK, IDOK); end;
end;
function DefaultDestination(Param: String): String;
begin
#ifdef FixtureRoot
  Result := '{#FixtureRoot}\program';
#else
  if (OldDirectory <> '') and (OldMachine = IsAdminInstallMode) then Result := OldDirectory
  else Result := ExpandConstant('{autopf}\Harmony');
#endif
end;
function ProgramsDirectory(Param: String): String;
begin
#ifdef FixtureRoot
  Result := '{#FixtureRoot}\shortcuts';
#else
  Result := ExpandConstant('{autoprograms}');
#endif
end;
function MayCreateShortcut: Boolean;
begin Result := not FileExists(AddBackslash(ProgramsDirectory('')) + 'Harmony.lnk'); end;
procedure InitializeWizard;
var Text, Scope: String;
begin
  if IsAdminInstallMode then Scope := 'all users' else Scope := 'your Windows account only';
  if DetectionError <> '' then Text := DetectionError
  else if OldDirectory = '' then Text := 'No registered previous Harmony installation was found.'
  else begin
    Text := 'Found Harmony ' + OldVersion + ' in:' + #13#10 + OldDirectory + #13#10#13#10;
    if OldMachine and not IsAdminInstallMode then Text := Text + 'Move to your account: the shared program will be removed. Other users keep their data but lose shared app access.'
    else Text := Text + 'Update the program and keep saved data.';
  end;
  Text := Text + #13#10#13#10 + 'Install for: ' + Scope + '.' + #13#10 + 'Profiles, settings and saved sign-ins stay where they are. Setup does not copy, reset or delete them.';
  CreateOutputMsgPage(wpWelcome, 'Harmony installation', 'Previous version and install scope', Text);
end;
function AppIsRunning: Boolean;
var Locator, Service, Processes, Process: Variant; I: Integer; Path: String;
begin
  Result := False; Locator := CreateOleObject('WbemScripting.SWbemLocator'); Service := Locator.ConnectServer('', 'root\CIMV2');
  Processes := Service.ExecQuery('SELECT ExecutablePath FROM Win32_Process WHERE Name="Harmony.exe" OR Name="yapfast.exe"');
  for I := 0 to Processes.Count - 1 do begin
    Process := Processes.ItemIndex(I);
    if not VarIsNull(Process.ExecutablePath) then begin
      Path := Process.ExecutablePath;
      if UnderDirectory(Path, WizardDirValue) or ((OldDirectory <> '') and UnderDirectory(Path, OldDirectory)) then Result := True;
    end;
  end;
end;
function CompatibilityDirectory(Param: String): String;
begin if CompatibilityCopy then Result := OldDirectory else Result := ExpandConstant('{app}'); end;
function CompatibilityName(Param: String): String;
begin if CompatibilityCopy then Result := OldExecutable else Result := 'Harmony.exe'; end;
function NeedsCompatibilityCopy: Boolean;
begin Result := CompatibilityCopy; end;
function PrepareToInstall(var NeedsRestart: Boolean): String;
var Exe, Params, Product, Owner: String; Code: Integer; Started, Retire: Boolean;
    Root: Integer; Context, Size, Status: LongWord;
begin
  Result := DetectionError; if Result <> '' then Exit;
  if not ProgramDirectory(WizardDirValue) then begin Result := 'Choose a program folder, not a data or Windows folder.'; Exit; end;
#ifdef FixtureRoot
  if not UnderDirectory(WizardDirValue, '{#FixtureRoot}') then begin Result := 'Private test destination required.'; Exit; end;
#endif
  if AppIsRunning then begin Result := 'Quit Harmony, including its tray icon, then click Install again. Your saved sign-in will be kept.'; Exit; end;
  Retire := (OldDirectory <> '') and not PreviousRemoved and ((OldKind = 'msi') or (OldMachine <> IsAdminInstallMode) or not SamePath(OldDirectory, WizardDirValue));
  if not Retire then Exit;
  CompatibilityCopy := OldMachine and IsAdminInstallMode and ((OldExecutable <> 'Harmony.exe') or not SamePath(OldDirectory, WizardDirValue));
  if OldKind = 'msi' then begin
    Exe := ExpandConstant('{sys}\msiexec.exe');
    Params := '/x ' + LegacyProduct + ' /qn /norestart REBOOT=ReallySuppress MSIRESTARTMANAGERCONTROL=Disable MSIDISABLERMRESTART=1';
  end else begin Exe := OldUninstaller; Params := '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART'; end;
  if OldMachine and not IsAdminInstallMode then Started := ShellExec('runas', Exe, Params, '', SW_HIDE, ewWaitUntilTerminated, Code)
  else if not OldMachine then Started := ExecAsOriginalUser(Exe, Params, '', SW_HIDE, ewWaitUntilTerminated, Code)
  else Started := Exec(Exe, Params, '', SW_HIDE, ewWaitUntilTerminated, Code);
  if not Started or (Code <> 0) then begin Result := 'Previous program removal failed (' + IntToStr(Code) + '). Setup stopped; saved data has not been removed.'; Exit; end;
  if OldMachine then Root := HKLM64 else Root := HKCU;
  if (OldKind = 'inno') and RegKeyExists(Root, UninstallKey) then begin Result := 'Previous program is still registered. Setup stopped; saved data is untouched.'; Exit; end;
  if OldKind = 'msi' then begin
    Product := StringOfChar(#0, 39); Owner := StringOfChar(#0, 256); Size := 255;
    Status := EnumMsi(LegacyProduct, 0, 7, 0, Product, Context, Owner, Size);
    if Status <> 259 then begin Result := 'Previous MSI removal could not be confirmed. Setup stopped; inspect Windows Apps before retrying.'; Exit; end;
    if FileExists(AddBackslash(OldDirectory) + OldExecutable) then begin Result := 'Previous program files remain. Setup stopped; saved data is untouched.'; Exit; end;
  end;
  PreviousRemoved := True;
end;
procedure CurStepChanged(Step: TSetupStep);
var Root: Integer;
begin
  if Step <> ssPostInstall then Exit;
  if IsAdminInstallMode then Root := HKLM64 else Root := HKCU;
  if not RegWriteStringValue(Root, UninstallKey, 'HarmonySimpleInstaller', '1') or
     not RegWriteStringValue(Root, UninstallKey, 'HarmonyUninstallerSHA256', GetSHA256OfFile(ExpandConstant('{uninstallexe}'))) then RaiseException('Could not record installer information.');
end;
function InitializeUninstall: Boolean;
begin
#ifdef FixtureRoot
  Result := UnderDirectory(ExpandConstant('{app}'), '{#FixtureRoot}');
#else
  Result := True;
#endif
end;
// No [Run], data ownership, startup/protocol changes or recursive deletion.
