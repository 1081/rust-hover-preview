“PDF‑XChange (Legacy)” in Rust Hover Preview refers to the old PDF‑XChange Viewer handler. It cannot be activated unless that legacy handler is actually installed and registered on the machine. Current PDF‑XChange Editor installations normally provide only “PDF‑XChange (Current).”
To enable the current handler:
Run as administrator:
C:\Program Files\PDF-XChange\Shell Extensions\XCShInfoSetup.exe
Older installations use:
C:\Program Files\Tracker Software\Shell Extensions\XCShInfoSetup.exe

Set all handlers to None, click Apply.

Select PDF‑XChange for the PDF preview handler, then click Apply again.

Restart Windows.

In Rust Hover Preview, choose PDF Preview Handler → PDF‑XChange (Current).

You can check whether the legacy handler exists with:
reg query "HKCR\CLSID\{DC6EFB56-9CFA-464D-8880-44885D7DC193}"
If Windows reports that the key does not exist, the legacy option will remain disabled. Installing the discontinued PDF‑XChange Viewer with its Shell Extensions component would register it, but I recommend using the current handler instead—especially since the legacy handler is absent on both machines.
If XCShInfoSetup.exe is missing, modify/reinstall PDF‑XChange Editor and ensure Windows Shell Extensions is selected. These steps follow PDF‑XChange’s official shell-extension setup and re-registration instructions.