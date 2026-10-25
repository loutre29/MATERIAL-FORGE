; Material Forge installer (Inno Setup 6).
; Build: ISCC.exe installer\material_forge.iss  (after cargo build --release)
; Output: dist\MaterialForge-Setup-<version>.exe

#define AppName "Material Forge"
#define AppVersion "0.2.0"
#define AppExe "material_forge.exe"

[Setup]
AppId={{6E0B7C1A-6F2D-4C55-9C0E-5A3D2F7B9E41}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Per-user install by default: no administrator rights needed.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=..\dist
OutputBaseFilename=MaterialForge-Setup-{#AppVersion}
SetupIconFile=..\assets\material_forge.ico
UninstallDisplayIcon={app}\{#AppExe}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "assoc"; Description: "Open Material Forge projects (.sprp) with {#AppName}"; GroupDescription: "File associations:"

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "TESTERS.txt"; DestDir: "{app}"; Flags: ignoreversion isreadme
; Texture materials of the Assets shelf (CC0, Poly Haven and ambientCG).
Source: "..\starter_assets\*"; DestDir: "{app}\starter_assets"; Flags: ignoreversion recursesubdirs createallsubdirs
; Intel Open Image Denoise (Apache 2.0): the render mode denoiser, loaded at runtime.
Source: "..\third_party\oidn\*"; DestDir: "{app}\oidn"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\Notes for testers"; Filename: "{app}\TESTERS.txt"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.sprp"; ValueType: string; ValueName: ""; ValueData: "MaterialForge.Project"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\MaterialForge.Project"; ValueType: string; ValueName: ""; ValueData: "Material Forge project"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\MaterialForge.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\MaterialForge.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: assoc

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
