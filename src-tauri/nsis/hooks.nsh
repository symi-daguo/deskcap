; DeskCap NSIS hooks — do not invent installer UX; only fix stale desktop icons.
;
; Tauri's CreateOrUpdateDesktopShortcut returns early when $UpdateMode = 1, so
; upgrades leave the old .lnk (and its cached square icon) forever. If a desktop
; shortcut already exists, delete + recreate it pointing at the new exe icon.

!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
    Delete "$DESKTOP\${PRODUCTNAME}.lnk"
    ; Explicit icon file + index 0 so Explorer does not keep a stale icon cache entry.
    CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "" "$INSTDIR\${MAINBINARYNAME}.exe" 0
  ${EndIf}
!macroend
