# PLAN

Completed items: `docs/PLAN_LOG.md`. Background: `docs/plan_context/`.

## Web application
- [ ] Mobile: responsive layout and touch (context: docs/plan_context/web-mobile.md)
	- [x] A slow pinch at a low zoom no longer stalls: `GridState::pinch_zoom` keeps the unrounded zoom between frames (`tests/touch.rs`) (done 2026-10-14 14:55 EDT)
	- [ ] Pin the OK/Cancel row of phone dialogs taller than the screen below their scroll area.
	- [ ] Show that the ribbon overflows (its scroll bar is hidden), with a fade or chevron at the clipped edge.
	- [ ] BDFN: check the touch ribbon, the iOS file picker and scrolling while editing on a phone and tablet.
	- [ ] BDFN: on an iPhone, check that double-tapping a cell brings up the keyboard (context: docs/plan_context/web-mobile.md)
	- [ ] Bring the tab row (30 pt) and title-bar buttons (26 pt) to 44 pt on touch, after BDFN's look.
	- [x] Thresholds hold on a real device as they are (tap slop 10 pt, tap-then-drag window 0.3 s and 30 pt, long press 0.5 s, momentum time constant 0.325 s); BDFN tried the deployed build and approved them with the phone layout (2026-10-13 12:28 PM EDT).
	- [x] Phone: the File view (backstage) left bar pushed the page content past the right edge (fixed 2026-10-13 9:50 PM EDT). Below the compact width the navigation becomes a row along the top that scrolls sideways; pages cap their width at the screen; Print stacks the preview under its settings.
	- [x] Told WordCraft about both fixes via llmsend (2026-10-13 9:58 PM EDT).
	- [x] Ribbon wheel panning, diagonal tail (found by WordCraft; fixed 2026-10-13 9:55 PM EDT): egui smooths each axis separately, so a diagonal trackpad gesture ends with a vertical-only tail that `wheel_pans_sideways` turns into extra sideways motion. Classify each raw wheel gesture as purely vertical or not, remember it while scrolling, and remap only pure vertical gestures. Port WordCraft's differential test (horizontal vs diagonal end at the same offset).
	- [x] Web picker hardening (2026-10-13 11:55 PM EDT): `apps/gridcraft-web/src/picker.rs` keeps one picker in a thread-local slot and drops the input and both handlers when either fires, or when a new pick replaces a picker the browser never answered. Files over 2 GiB (Excel's own file limit) are refused with a message box, and the copy reserves memory fallibly so a file the page can't hold is a message rather than an abort. Covered by headless-Chromium tests in `checks.web-storage`.
	- [x] Ribbon accessible names (2026-10-14 12:15 AM EDT): every ribbon control is announced by name (`widgets::announce`): a button's visible label, else its tooltip without the trailing note in parentheses ("Cut (⌘X)" is "Cut"); toggles report on/off; a split button's arrow is "<name> menu". `tests/ribbon_accessibility.rs` sweeps every tab for unnamed clickable nodes (156 before). Tests that found a control by a name the title bar or ribbon now shares pick the last-drawn match.
	- [x] Touch-sized ribbon (2026-10-14 12:30 AM EDT): `TouchTranslator::coarse` turns on with a finger and off with a mouse hover or wheel with no finger down. While it is on, every ribbon control is at least 44 points square (`widgets::TOUCH_TARGET`, Apple's minimum), the three-high stacks become one row (`ribbon::stack`), and egui's checkboxes and combo boxes in the ribbon size to the same interact size. The title bar and formula bar keep their fixed heights. Tests: the classifier over a set of input runs in `touch.rs`, and `tests/touch_targets.rs` (every tab swept, plus mouse return and fixed bars). `--touch` on the snapshot example renders the layout.
	- [x] Scrolling no longer closes or reopens the software keyboard (2026-10-14 12:50 PM EDT). Two causes, each with a failing test first. (1) egui's text field asks for text input only while visible, so scrolling the edited cell out of view dropped it and eframe blurred its hidden input; the wheel did it too, and desktop builds lost the IME. `grid::keep_text_input` keeps asking while the in-cell editor has the focus (`tests/editing_scroll.rs`, wheel and finger). (2) eframe 0.36.2 focuses the canvas on `pointerup` and blurs and refocuses its text input on `touchend`: three cycles per drag, as WordCraft found. `apps/gridcraft-web/src/touch_focus.rs` hides both from eframe while the fingers scroll or pinch and delivers the touch endings itself; taps still go through eframe. Browser tests start the real web runner in headless Chromium and record every focus and blur.
	- [x] Real-device check: BDFN tried the deployed build and found nothing to change (2026-10-13 12:28 PM EDT).
- [ ] Web end-to-end: drive the whole app in Chromium (two tabs, reload, Save-and-close, restart the profile and reopen) (context: docs/plan_context/web-mobile.md)
- [ ] Web fonts: check in a real browser that a Calibri workbook fetches Carlito and renders with it.
- [ ] Web fonts: harden the fetched-face boundary (context: docs/plan_context/web-mobile.md)
- [ ] Web fonts, later: craft-fonts' script-fallback faces (CJK is many MB) loaded only when a workbook uses that script; fonts the user imports into browser storage.

## Interface languages
- [ ] Translate the UI into the i18n skill's 50-locale baseline, mirroring the whole layout for right-to-left locales (context: docs/plan_context/interface-languages.md)

## Build and distribution
- [ ] Seal the GridCraft.app bundle signature in Nix with rcodesign, verified by Apple's codesign on a Mac (context: docs/plan_context/macos-signing.md)
- [ ] Follow the fork's publishing policy after each upstream sync (context: docs/plan_context/fork-workflow.md)
