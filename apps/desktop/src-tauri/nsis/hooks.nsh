; BlockHost custom NSIS hooks
; Safety rule: uninstalling BlockHost must ask whether to keep Minecraft worlds and backups
; and default to keeping them so players never lose a world by accident.

!macro NSIS_HOOK_POSTUNINSTALL
  IfFileExists "$LOCALAPPDATA\BlockHost\*.*" 0 +4
    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON1 "Keep your Minecraft worlds, backups, and server settings in $LOCALAPPDATA\BlockHost?$\r$\n$\r$\nChoose Yes (recommended) to keep your worlds safe, or No to delete everything." IDYES +2
    RMDir /r "$LOCALAPPDATA\BlockHost"
!macroend
