; Instalador de Windows para Eginbeharraren deia (Inno Setup 6).
; Lo compila el flujo de GitHub Actions; a mano: ISCC.exe /DVersion=0.2.0 eginbeharraren-deia.iss

#ifndef Version
  #define Version "0.2.0"
#endif

[Setup]
AppId={{8C1F4A44-1944-4F44-9A44-F4E44F4E4444}
AppName=Eginbeharraren deia
AppVersion={#Version}
AppPublisher=Eginbeharraren deia
DefaultDirName={autopf}\Eginbeharraren deia
DefaultGroupName=Eginbeharraren deia
DisableProgramGroupPage=yes
OutputDir=..\..\dist
OutputBaseFilename=EginbeharrarenDeia-{#Version}-Windows-Instalador
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\eginbeharraren-deia.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\..\target\release\eginbeharraren-deia.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\eginbeharraren-deia-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\icon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "servidor-dedicado.bat"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\modelos\*"; DestDir: "{app}\assets\modelos"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Eginbeharraren deia"; Filename: "{app}\eginbeharraren-deia.exe"; IconFilename: "{app}\icon.ico"
Name: "{group}\Servidor dedicado de Eginbeharraren deia"; Filename: "{app}\servidor-dedicado.bat"; WorkingDir: "{app}"; IconFilename: "{app}\icon.ico"
Name: "{group}\Desinstalar Eginbeharraren deia"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Eginbeharraren deia"; Filename: "{app}\eginbeharraren-deia.exe"; IconFilename: "{app}\icon.ico"; Tasks: desktopicon

[Run]
; Permitir conexiones entrantes en el cortafuegos para poder alojar partidas
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Eginbeharraren deia"" dir=in action=allow protocol=UDP program=""{app}\eginbeharraren-deia.exe"" enable=yes"; Flags: runhidden
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Eginbeharraren deia (servidor)"" dir=in action=allow protocol=UDP program=""{app}\eginbeharraren-deia-server.exe"" enable=yes"; Flags: runhidden
Filename: "{app}\eginbeharraren-deia.exe"; Description: "{cm:LaunchProgram,Eginbeharraren deia}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Eginbeharraren deia"""; Flags: runhidden; RunOnceId: "fw1"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Eginbeharraren deia (servidor)"""; Flags: runhidden; RunOnceId: "fw2"
