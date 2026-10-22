; Installs the bundled VST3 into the standard per-machine VST3 folder and removes it on uninstall.
; Also owns two inbound Windows Firewall allow rules (TCP and UDP 43101) scoped to the installed app binary, replacing
; any earlier inbound rule for that binary (including a block left by a dismissed Windows prompt).
; Each netsh call has a timeout: a stalled firewall service must not leave the install page
; waiting forever. A failed or timed-out rule only means Windows may ask about the firewall later.
!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Installing the TrackMesh VST3 plugin..."
  CreateDirectory "$COMMONFILES64\VST3"
  RMDir /r "$COMMONFILES64\VST3\TrackMesh.vst3"
  CopyFiles /SILENT "$INSTDIR\vst3\TrackMesh.vst3" "$COMMONFILES64\VST3"
  DetailPrint "Removing earlier Windows Firewall rules for TrackMesh..."
  nsExec::Exec /TIMEOUT=30000 '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name=all dir=in program="$INSTDIR\trackmesh-desktop.exe"'
  Pop $0
  DetailPrint "Firewall cleanup result: $0"
  DetailPrint "Allowing TrackMesh in Windows Firewall..."
  ; Only the default listen port 43101: TCP for libp2p, UDP for QUIC.
  nsExec::Exec /TIMEOUT=30000 '"$SYSDIR\netsh.exe" advfirewall firewall add rule name="TrackMesh" dir=in action=allow program="$INSTDIR\trackmesh-desktop.exe" protocol=TCP localport=43101 enable=yes profile=any'
  Pop $0
  DetailPrint "Firewall TCP rule result: $0"
  nsExec::Exec /TIMEOUT=30000 '"$SYSDIR\netsh.exe" advfirewall firewall add rule name="TrackMesh" dir=in action=allow program="$INSTDIR\trackmesh-desktop.exe" protocol=UDP localport=43101 enable=yes profile=any'
  Pop $0
  DetailPrint "Firewall UDP rule result: $0"
  ; Go straight to the Finish page when the files are in place, instead of stopping on
  ; "Completed" with a Next button.
  SetAutoClose true
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  RMDir /r "$COMMONFILES64\VST3\TrackMesh.vst3"
  DetailPrint "Removing Windows Firewall rules for TrackMesh..."
  nsExec::Exec /TIMEOUT=30000 '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name=all dir=in program="$INSTDIR\trackmesh-desktop.exe"'
  Pop $0
  SetAutoClose true
!macroend
