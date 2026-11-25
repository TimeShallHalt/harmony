; Harmony's Windows installer. The maintenance helper runs as the initiating user.
#ifndef PayloadFiles
  #error Build with scripts/build-harmony-installer.py
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
VersionInfoDescription=Harmony Installer with Repair
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
SetupMutex=Global\HarmonySetup-{#HarmonyId}
UninstallDisplayIcon={app}\Harmony.exe
Uninstallable=InstallPayload
LicenseFile={#PayloadDir}\LICENSE.txt
OutputBaseFilename=Harmony-{#AppVersion}-Setup
Compression=lzma2/normal
SolidCompression=yes
CloseApplications=no
RestartApplications=no
AlwaysRestart=no
RestartIfNeededByRun=no

[Messages]
PrivilegesRequiredOverrideTitle=Harmony Setup
PrivilegesRequiredOverrideInstruction=Choose an installation scope
PrivilegesRequiredOverrideText1=The all-users option installs a shared copy. Choose me only to install for your account or repair existing copies.
PrivilegesRequiredOverrideText2=The me-only option installs for your account or repairs existing copies. The all-users option installs a shared copy.
PrivilegesRequiredOverrideAllUsers=Install / update for &all users
PrivilegesRequiredOverrideAllUsersRecommended=Install / update for &all users
PrivilegesRequiredOverrideCurrentUser=Install or repair for &me only
PrivilegesRequiredOverrideCurrentUserRecommended=Install or repair for &me only

[Files]
Source: "{#RepairTool}"; Flags: dontcopy noencryption
#include PayloadFiles

[Icons]
Name: "{code:ProgramsDirectory}\Harmony"; Filename: "{app}\Harmony.exe"; WorkingDir: "{app}"; Check: MayCreateShortcut

[Code]
const
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{{#HarmonyId}}_is1';
var
  Maintenance: TInputOptionWizardPage;
  ExplicitRepair, Prepared: Boolean;
  OldDirectory, Keeper, KeeperVersion, ScanError, LastReport, FinishError: String;
  Count, Compare, Changes: Integer;
  SameScope: Boolean;

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
  if Handle = 0 then Exit;
  try
    if WaitForParent(Handle, 60000) <> 0 then RaiseException('Harmony is still running. Quit it and try again.');
  finally CloseParent(Handle); end;
end;

function InitializeSetup: Boolean;
var I: Integer;
begin
  Result := False;
  try
    for I := 1 to ParamCount do
      if CompareText(ParamStr(I), '/REPAIR') = 0 then ExplicitRepair := True;
    WaitForHarmony;
    Result := True;
  except SuppressibleMsgBox(GetExceptionMessage, mbError, MB_OK, IDOK); end;
end;

function RepairOnly: Boolean;
begin
  Result := ExplicitRepair;
  if Maintenance <> nil then Result := Result or (Maintenance.SelectedValueIndex = 1);
end;

function InstallPayload: Boolean;
begin Result := not RepairOnly; end;

function Scope: String;
begin if IsAdminInstallMode then Result := 'all' else Result := 'user'; end;

function Field(Name: String): String;
begin Result := GetIniString('repair', Name, '', LastReport); end;

function CallMaintenance(Mode, Target: String): String;
var Params, Expected, Helper: String; Code, I: Integer;
begin
  LastReport := ExpandConstant('{tmp}\harmony-' + Mode + '.ini');
  if FileExists(LastReport) and not DeleteFile(LastReport) then RaiseException('Could not refresh the Setup report.');
  Helper := ExpandConstant('{tmp}\HarmonyRepair.exe');
  Expected := ExpandConstant('{param:HARMONYINSTALLED|}');
  if (Pos('"', Target) <> 0) or (Pos('"', Expected) <> 0) then RaiseException('Invalid quoted program path.');
  Params := Mode + ' "' + LastReport + '" ' + Scope + ' "' + Target + '" "' + Expected + '" "' + ExpandConstant('{tmp}\harmony-old-paths.txt') + '"';
  Result := '';
  if not ExecAsOriginalUser(Helper, Params, ExpandConstant('{tmp}'), SW_HIDE, ewWaitUntilTerminated, Code) then
    Result := 'Windows could not start Harmony maintenance (' + IntToStr(Code) + ').'
  else if (Code <> 0) or (Field('status') <> '0') then begin
    Result := Field('error');
    if Result = '' then Result := 'Could not read the maintenance result. Start Setup from your Windows account. Code ' + IntToStr(Code) + '.';
  end;
  Log('Harmony ' + Mode + ' status=' + Field('status') + ' keeper=' + Field('keeper') +
    ' removed=' + Field('removed') + ' launchers=' + Field('launchers') + ' error=' + Result);
  for I := 0 to StrToIntDef(Field('count'), 0) - 1 do Log('Harmony installed ' + Field('copy' + IntToStr(I)));
  I := 0;
  while Field('edit' + IntToStr(I)) <> '' do begin Log('Harmony plan: ' + Field('edit' + IntToStr(I))); I := I + 1; end;
end;

function DefaultDestination(Param: String): String;
begin
  if (OldDirectory <> '') and SameScope then Result := OldDirectory
#ifdef FixtureRoot
  else Result := '{#FixtureRoot}\program';
#else
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
begin Result := InstallPayload and not FileExists(AddBackslash(ProgramsDirectory('')) + 'Harmony.lnk'); end;

procedure InitializeWizard;
var Text: String; I: Integer; SuppliedDir: Boolean;
begin
  ExtractTemporaryFile('HarmonyRepair.exe');
  ScanError := CallMaintenance('scan', '');
  OldDirectory := Field('previous'); SameScope := Field('sameScope') = '1';
  Keeper := Field('keeper'); KeeperVersion := Field('version');
  Count := StrToIntDef(Field('count'), 0); Compare := StrToIntDef(Field('compare'), -1);
  Changes := StrToIntDef(Field('changes'), 0);
  if ScanError <> '' then Text := ScanError
  else if Count = 0 then Text := 'No installed Harmony was found.'
  else begin
    Text := 'Installed on this PC:' + #13#10;
    for I := 0 to Count - 1 do Text := Text + Field('copy' + IntToStr(I)) + #13#10;
    Text := Text + #13#10 + 'Repair keeps ' + KeeperVersion + ' in ' + Keeper + '.';
    if Changes > 0 then Text := Text + ' Copies and launchers to clean up: ' + IntToStr(Changes) + '.';
  end;
  Maintenance := CreateInputOptionPage(wpWelcome, 'Install or repair Harmony', 'Choose what Setup should do.', Text, True, False);
  Maintenance.Add('Install or update to {#AppVersion} build {#AppBuild} and clean up older copies');
  Maintenance.Add('Repair installed copies and keep the newest version');
  if ExplicitRepair or ((not WizardSilent) and ((Compare > 0) or ((Compare = 0) and (Changes > 0)))) then Maintenance.SelectedValueIndex := 1
  else Maintenance.SelectedValueIndex := 0;
  SuppliedDir := False;
  for I := 1 to ParamCount do if Pos('/DIR=', Uppercase(ParamStr(I))) = 1 then SuppliedDir := True;
  if (OldDirectory <> '') and SameScope and not SuppliedDir then WizardForm.DirEdit.Text := OldDirectory;
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := RepairOnly and ((PageID = wpLicense) or (PageID = wpSelectDir) or (PageID = wpSelectProgramGroup));
end;

function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo, MemoTypeInfo, MemoComponentsInfo, MemoGroupInfo, MemoTasksInfo: String): String;
begin
  if RepairOnly then Result := 'Repair existing Harmony installations.' + NewLine + NewLine + 'Keep ' + KeeperVersion + ':' + NewLine + Keeper + NewLine + NewLine +
    'Repair removes verified older programs and duplicate Start entries. Existing pins and Discord links keep their arguments. Profiles and saved sign-ins stay in place.'
  else begin
    Result := 'Install Harmony {#AppVersion} build {#AppBuild} in:' + NewLine + WizardDirValue + NewLine + NewLine +
      'Setup will remove superseded installations and repair existing launchers. Profiles and saved sign-ins stay in place.';
    if (OldDirectory <> '') and not SameScope and not IsAdminInstallMode then Result := Result + NewLine + NewLine + 'Moving to your account removes the shared installation for other users.';
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := ScanError;
  if Result <> '' then Exit;
  if RepairOnly then begin
    WizardForm.DirEdit.Text := ExpandConstant('{tmp}');
    Result := CallMaintenance('repair', '');
  end
  else if not Prepared then begin
    Result := CallMaintenance('prepare', WizardDirValue);
    Prepared := Result = '';
  end;
end;

procedure CurStepChanged(Step: TSetupStep);
var Root: Integer;
begin
  if (Step <> ssPostInstall) or RepairOnly then Exit;
  if IsAdminInstallMode then Root := HKLM64 else Root := HKCU;
  try
    if not RegWriteStringValue(Root, UninstallKey, 'HarmonySimpleInstaller', '1') or
       not RegWriteStringValue(Root, UninstallKey, 'HarmonyUninstallerSHA256', GetSHA256OfFile(ExpandConstant('{uninstallexe}'))) then RaiseException('Could not record installer information.');
    FinishError := CallMaintenance('finish', ExpandConstant('{app}'));
  except FinishError := GetExceptionMessage; end;
  if FinishError <> '' then begin
    Log('Harmony cleanup incomplete: ' + FinishError);
    SuppressibleMsgBox(FinishError + #13#10 + 'Run Repair to finish cleanup.', mbError, MB_OK, IDOK);
  end;
end;

function GetCustomSetupExitCode: Integer;
begin if FinishError <> '' then Result := 11 else Result := 0; end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if RepairOnly and (CurPageID = wpReady) then WizardForm.NextButton.Caption := '&Repair';
  if RepairOnly and (CurPageID = wpFinished) then begin
    WizardForm.FinishedHeadingLabel.Caption := 'Harmony repair complete';
    WizardForm.FinishedLabel.Caption := 'Kept Harmony ' + KeeperVersion + ' in ' + Keeper + '.';
  end;
  if (FinishError <> '') and (CurPageID = wpFinished) then begin
    WizardForm.FinishedHeadingLabel.Caption := 'Harmony still needs repair';
    WizardForm.FinishedLabel.Caption := FinishError;
  end;
end;

function InitializeUninstall: Boolean;
begin
#ifdef FixtureRoot
  Result := Pos(Lowercase(AddBackslash('{#FixtureRoot}')), Lowercase(ExpandConstant('{app}'))) = 1;
#else
  Result := True;
#endif
end;
