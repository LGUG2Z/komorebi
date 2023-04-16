; Generated by komorebic.exe
; To use this file, add the line below to the top of your komorebi.ahk configuration file
; #Include %A_ScriptDir%\komorebi.generated.ahk

; 1Password
RunWait("komorebic.exe float-rule exe '1Password.exe'", , "Hide")

; Ableton Live
; Targets VST2 windows
RunWait("komorebic.exe float-rule class 'AbletonVstPlugClass'", , "Hide")
; Targets VST3 windows
RunWait("komorebic.exe float-rule class 'Vst3PlugWindow'", , "Hide")

; Adobe Creative Cloud
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application class 'CreativeCloudDesktopWindowClass'", , "Hide")

; Adobe Photoshop
RunWait("komorebic.exe identify-border-overflow-application class 'Photoshop'", , "Hide")

; ArmCord
RunWait("komorebic.exe identify-border-overflow-application exe 'ArmCord.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'ArmCord.exe'", , "Hide")

; AutoHotkey
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'AutoHotkeyU64.exe'", , "Hide")
RunWait("komorebic.exe float-rule title 'Window Spy'", , "Hide")

; Beeper
RunWait("komorebic.exe identify-border-overflow-application exe 'Beeper.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'Beeper.exe'", , "Hide")

; Bitwarden
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'Bitwarden.exe'", , "Hide")

; Bloxstrap
RunWait("komorebic.exe float-rule exe 'Bloxstrap.exe'", , "Hide")

; Calculator
RunWait("komorebic.exe float-rule title 'Calculator'", , "Hide")

; Credential Manager UI Host
; Targets the Windows popup prompting you for a PIN instead of a password on 1Password etc.
RunWait("komorebic.exe float-rule exe 'CredentialUIBroker.exe'", , "Hide")

; Cron
RunWait("komorebic.exe identify-border-overflow-application exe 'Cron.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'Cron.exe'", , "Hide")

; Delphi applications
; Target hidden window spawned by Delphi applications
RunWait("komorebic.exe float-rule class 'TApplication'", , "Hide")
; Target Inno Setup installers
RunWait("komorebic.exe float-rule class 'TWizardForm'", , "Hide")

; Discord
RunWait("komorebic.exe identify-border-overflow-application exe 'Discord.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'Discord.exe'", , "Hide")

; DiscordCanary
RunWait("komorebic.exe identify-border-overflow-application exe 'DiscordCanary.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'DiscordCanary.exe'", , "Hide")

; DiscordDevelopment
RunWait("komorebic.exe identify-border-overflow-application exe 'DiscordDevelopment.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'DiscordDevelopment.exe'", , "Hide")

; DiscordPTB
RunWait("komorebic.exe identify-border-overflow-application exe 'DiscordPTB.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'DiscordPTB.exe'", , "Hide")

; Dropbox
RunWait("komorebic.exe float-rule exe 'Dropbox.exe'", , "Hide")

; ElectronMail
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'ElectronMail.exe'", , "Hide")

; Element
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'Element.exe'", , "Hide")

; Elephicon
RunWait("komorebic.exe float-rule exe 'Elephicon.exe'", , "Hide")

; ElevenClock
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'ElevenClock.exe'", , "Hide")

; Elgato Camera Hub
RunWait("komorebic.exe float-rule exe 'Camera Hub.exe'", , "Hide")

; Elgato Control Center
RunWait("komorebic.exe float-rule exe 'ControlCenter.exe'", , "Hide")

; Elgato Wave Link
RunWait("komorebic.exe float-rule exe 'WaveLink.exe'", , "Hide")

; Epic Games Launcher
RunWait("komorebic.exe identify-border-overflow-application exe 'EpicGamesLauncher.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'EpicGamesLauncher.exe'", , "Hide")

; Flow Launcher
RunWait("komorebic.exe identify-border-overflow-application exe 'Flow.Launcher.exe'", , "Hide")

; GOG Galaxy
RunWait("komorebic.exe identify-border-overflow-application exe 'GalaxyClient.exe'", , "Hide")
RunWait("komorebic.exe manage-rule exe 'GalaxyClient.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'GalaxyClient.exe'", , "Hide")
; Targets a hidden window spawned by GOG Galaxy
RunWait("komorebic.exe float-rule class 'Chrome_RenderWidgetHostHWND'", , "Hide")

; GoPro Webcam
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application class 'GoPro Webcam'", , "Hide")

; Godot Manager
RunWait("komorebic.exe identify-border-overflow-application exe 'GodotManager.exe'", , "Hide")
RunWait("komorebic.exe manage-rule exe 'GodotManager.exe'", , "Hide")
RunWait("komorebic.exe identify-object-name-change-application exe 'GodotManager.exe'", , "Hide")

; Google Chrome
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'chrome.exe'", , "Hide")

; Google Drive
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'GoogleDriveFS.exe'", , "Hide")

; Houdoku
RunWait("komorebic.exe identify-border-overflow-application exe 'Houdoku.exe'", , "Hide")

; IntelliJ IDEA
RunWait("komorebic.exe identify-object-name-change-application exe 'idea64.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'idea64.exe'", , "Hide")
; Targets JetBrains IDE popups and floating windows
RunWait("komorebic.exe float-rule class 'SunAwtDialog'", , "Hide")

; Itch.io
RunWait("komorebic.exe identify-border-overflow-application exe 'itch.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'itch.exe'", , "Hide")

; Keyviz
RunWait("komorebic.exe float-rule exe 'keyviz.exe'", , "Hide")

; Kleopatra
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'kleopatra.exe'", , "Hide")

; Kotatogram
RunWait("komorebic.exe identify-border-overflow-application exe 'Kotatogram.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'Kotatogram.exe'", , "Hide")

; LocalSend
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'localsend_app.exe'", , "Hide")

; Logi Bolt
RunWait("komorebic.exe float-rule exe 'LogiBolt.exe'", , "Hide")

; LogiTune
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'LogiTune.exe'", , "Hide")
RunWait("komorebic.exe float-rule exe 'LogiTune.exe'", , "Hide")

; Logitech G HUB
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'lghub.exe'", , "Hide")
RunWait("komorebic.exe identify-border-overflow-application exe 'lghub.exe'", , "Hide")

; Logitech Options
RunWait("komorebic.exe float-rule exe 'LogiOptionsUI.exe'", , "Hide")

; Mailspring
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'mailspring.exe'", , "Hide")

; ManyCam
RunWait("komorebic.exe identify-border-overflow-application exe 'ManyCam.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'ManyCam.exe'", , "Hide")

; Mica For Everyone

; Microsoft Excel
RunWait("komorebic.exe identify-border-overflow-application exe 'EXCEL.EXE'", , "Hide")
RunWait("komorebic.exe identify-layered-application exe 'EXCEL.EXE'", , "Hide")
; Targets a hidden window spawned by Microsoft Office applications
RunWait("komorebic.exe float-rule class '_WwB'", , "Hide")

; Microsoft Outlook
RunWait("komorebic.exe identify-border-overflow-application exe 'OUTLOOK.EXE'", , "Hide")
RunWait("komorebic.exe identify-layered-application exe 'OUTLOOK.EXE'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'OUTLOOK.EXE'", , "Hide")

; Microsoft PC Manager
RunWait("komorebic.exe float-rule exe 'MSPCManager.exe'", , "Hide")

; Microsoft PowerPoint
RunWait("komorebic.exe identify-border-overflow-application exe 'POWERPNT.EXE'", , "Hide")
RunWait("komorebic.exe identify-layered-application exe 'POWERPNT.EXE'", , "Hide")

; Microsoft Teams
RunWait("komorebic.exe identify-border-overflow-application exe 'Teams.exe'", , "Hide")
; Target Teams pop-up notification windows
RunWait("komorebic.exe float-rule title 'Microsoft Teams Notification'", , "Hide")
; Target Teams call in progress windows
RunWait("komorebic.exe float-rule title 'Microsoft Teams Call'", , "Hide")

; Microsoft Word
RunWait("komorebic.exe identify-border-overflow-application exe 'WINWORD.EXE'", , "Hide")
RunWait("komorebic.exe identify-layered-application exe 'WINWORD.EXE'", , "Hide")

; Modern Flyouts
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'ModernFlyoutsHost.exe'", , "Hide")

; Mozilla Firefox
RunWait("komorebic.exe identify-object-name-change-application exe 'firefox.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'firefox.exe'", , "Hide")
; Targets invisible windows spawned by Firefox to show tab previews in the taskbar
RunWait("komorebic.exe float-rule class 'MozillaTaskbarPreviewClass'", , "Hide")

; NVIDIA GeForce Experience
RunWait("komorebic.exe identify-border-overflow-application exe 'NVIDIA GeForce Experience.exe'", , "Hide")

; NiceHash Miner
RunWait("komorebic.exe identify-border-overflow-application exe 'nhm_app.exe'", , "Hide")
RunWait("komorebic.exe manage-rule exe 'nhm_app.exe'", , "Hide")

; NohBoard
RunWait("komorebic.exe float-rule exe 'NohBoard.exe'", , "Hide")

; Notion Enhanced
RunWait("komorebic.exe identify-border-overflow-application exe 'Notion Enhanced.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'Notion Enhanced.exe'", , "Hide")

; OBS Studio (32-bit)
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'obs32.exe'", , "Hide")

; OBS Studio (64-bit)
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'obs64.exe'", , "Hide")

; ONLYOFFICE Editors
RunWait("komorebic.exe identify-border-overflow-application class 'DocEditorsWindowClass'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application class 'DocEditorsWindowClass'", , "Hide")

; Obsidian
RunWait("komorebic.exe identify-border-overflow-application exe 'Obsidian.exe'", , "Hide")
RunWait("komorebic.exe manage-rule exe 'Obsidian.exe'", , "Hide")

; OpenRGB
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'OpenRGB.exe'", , "Hide")

; Paradox Launcher
RunWait("komorebic.exe float-rule exe 'Paradox Launcher.exe'", , "Hide")

; Plexamp
RunWait("komorebic.exe identify-border-overflow-application exe 'Plexamp.exe'", , "Hide")

; PowerToys
; Target color picker dialog
RunWait("komorebic.exe float-rule exe 'PowerToys.ColorPickerUI.exe'", , "Hide")
; Target image resizer dialog
RunWait("komorebic.exe float-rule exe 'PowerToys.ImageResizer.exe'", , "Hide")

; Process Hacker
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'ProcessHacker.exe'", , "Hide")
RunWait("komorebic.exe float-rule exe 'ProcessHacker.exe'", , "Hide")

; ProtonVPN
RunWait("komorebic.exe identify-border-overflow-application exe 'ProtonVPN.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'ProtonVPN.exe'", , "Hide")

; PyCharm
RunWait("komorebic.exe identify-object-name-change-application exe 'pycharm64.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'pycharm64.exe'", , "Hide")

; QtScrcpy
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'QtScrcpy.exe'", , "Hide")

; QuickLook
RunWait("komorebic.exe float-rule exe 'QuickLook.exe'", , "Hide")

; RepoZ
RunWait("komorebic.exe float-rule exe 'RepoZ.exe'", , "Hide")

; Rider
RunWait("komorebic.exe identify-object-name-change-application exe 'rider64.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'rider64.exe'", , "Hide")

; Roblox FPS Unlocker
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'rbxfpsunlocker.exe'", , "Hide")

; RoundedTB
RunWait("komorebic.exe float-rule exe 'RoundedTB.exe'", , "Hide")

; RoundedTB
RunWait("komorebic.exe identify-border-overflow-application exe 'RoundedTB.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'RoundedTB.exe'", , "Hide")

; ShareX
RunWait("komorebic.exe identify-border-overflow-application exe 'ShareX.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'ShareX.exe'", , "Hide")

; Sideloadly
RunWait("komorebic.exe float-rule exe 'sideloadly.exe'", , "Hide")

; Signal
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'signal.exe'", , "Hide")

; SiriKali
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'sirikali.exe'", , "Hide")

; Slack
RunWait("komorebic.exe identify-border-overflow-application exe 'Slack.exe'", , "Hide")
RunWait("komorebic.exe manage-rule exe 'Slack.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'Slack.exe'", , "Hide")

; Slack
RunWait("komorebic.exe identify-border-overflow-application exe 'slack.exe'", , "Hide")
RunWait("komorebic.exe manage-rule exe 'slack.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'slack.exe'", , "Hide")

; Smart Install Maker
; Target hidden window spawned by installer
RunWait("komorebic.exe float-rule class 'obj_App'", , "Hide")
; Target installer
RunWait("komorebic.exe float-rule class 'obj_Form'", , "Hide")

; SoulseekQt
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'SoulseekQt.exe'", , "Hide")

; Spotify
RunWait("komorebic.exe identify-border-overflow-application exe 'Spotify.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'Spotify.exe'", , "Hide")

; Steam
RunWait("komorebic.exe identify-border-overflow-application class 'vguiPopupWindow'", , "Hide")

; Stremio
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'stremio.exe'", , "Hide")

; System Informer
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'SystemInformer.exe'", , "Hide")
RunWait("komorebic.exe float-rule exe 'SystemInformer.exe'", , "Hide")

; SystemSettings
RunWait("komorebic.exe float-rule class 'Shell_Dialog'", , "Hide")

; Task Manager
RunWait("komorebic.exe float-rule class 'TaskManagerWindow'", , "Hide")

; Telegram
RunWait("komorebic.exe identify-border-overflow-application exe 'Telegram.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'Telegram.exe'", , "Hide")

; TouchCursor
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'tcconfig.exe'", , "Hide")
RunWait("komorebic.exe float-rule exe 'tcconfig.exe'", , "Hide")

; TranslucentTB
RunWait("komorebic.exe float-rule exe 'TranslucentTB.exe'", , "Hide")

; TranslucentTB
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'TranslucentTB.exe'", , "Hide")

; Unreal Editor
RunWait("komorebic.exe identify-border-overflow-application exe 'UnrealEditor.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'UnrealEditor.exe'", , "Hide")

; VRCX
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'VRCX.exe'", , "Hide")

; Visual Studio
RunWait("komorebic.exe identify-object-name-change-application exe 'devenv.exe'", , "Hide")

; Visual Studio Code
RunWait("komorebic.exe identify-border-overflow-application exe 'Code.exe'", , "Hide")

; Voice.ai
RunWait("komorebic.exe identify-border-overflow-application exe 'VoiceAI.exe'", , "Hide")
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'VoiceAI.exe'", , "Hide")

; WebTorrent Desktop
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'WebTorrent.exe'", , "Hide")

; WinZip (32-bit)
RunWait("komorebic.exe float-rule exe 'winzip32.exe'", , "Hide")

; WinZip (64-bit)
RunWait("komorebic.exe float-rule exe 'winzip64.exe'", , "Hide")

; Windows Console (conhost.exe)
RunWait("komorebic.exe manage-rule class 'ConsoleWindowClass'", , "Hide")

; Windows Explorer
; Targets copy/move operation windows
RunWait("komorebic.exe float-rule class 'OperationStatusWindow'", , "Hide")
RunWait("komorebic.exe float-rule title 'Control Panel'", , "Hide")

; Windows Installer
RunWait("komorebic.exe float-rule exe 'msiexec.exe'", , "Hide")

; WingetUI
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'WingetUI.exe'", , "Hide")

; WingetUI
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'wingetui.exe'", , "Hide")

; Wox
; Targets a hidden window spawned by Wox
RunWait("komorebic.exe float-rule title 'Hotkey sink'", , "Hide")

; XAMPP Control Panel
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'xampp-control.exe'", , "Hide")

; Zoom
RunWait("komorebic.exe float-rule exe 'Zoom.exe'", , "Hide")

; mpv.net
RunWait("komorebic.exe identify-object-name-change-application exe 'mpvnet.exe'", , "Hide")

; paint.net
RunWait("komorebic.exe float-rule exe 'paintdotnet.exe'", , "Hide")

; pinentry
RunWait("komorebic.exe float-rule exe 'pinentry.exe'", , "Hide")

; qBittorrent
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'qbittorrent.exe'", , "Hide")

; ueli
; If you have disabled minimize/close to tray for this application, you can delete/comment out the next line
RunWait("komorebic.exe identify-tray-application exe 'ueli.exe'", , "Hide")
RunWait("komorebic.exe float-rule exe 'ueli.exe'", , "Hide")
