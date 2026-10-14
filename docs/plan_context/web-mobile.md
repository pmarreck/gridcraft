# Web and mobile notes

- Mobile direction (BDFN, 2026-10-12): responsive layout and touch. Touch gestures live in ui-egui and react only to real touch events, so mouse and trackpad are unchanged and native touchscreens benefit too.
- Touch thresholds approved on a real device 2026-10-13: tap slop 10 pt, tap-then-drag window 0.3 s and 30 pt, long press 0.5 s, momentum time constant 0.325 s.
- iOS file picker: WebKit opens a file input only while the browser is handling the tap, so a tap clicks in the frame eframe runs inside `pointerup`, and the web shell clicks its own file input synchronously.
- Double-tap to edit: the edit begins on a later frame, outside the touch handler, and Safari may refuse to focus the text input then. WordCraft activates typing inside the handler on a completed tap.
- Touch ribbon: while touch is active, ribbon controls are at least 44 points square; the tab row (30 points tall) and title-bar buttons (26 points) are still smaller.
- End-to-end browser tests: WordCraft's `nix/browser-test.nix` sets FONTCONFIG_FILE via makeFontsConf and uses CDP `Page.setInterceptFileChooserDialog` for imports. WordCraft also has a Chromium test that picks Lato and sees all four face requests.
- Font boundary hardening ideas (from WordCraft): cap fetched response size; reject a fetched face whose family name differs from the manifest; reject collections (TTC) and variable fonts at `add_loaded`.
