# Interface languages

Goal (requested by BDFN 2026-10-13): the 50-locale baseline from the shared `i18n` skill, with right-to-left locales (`ar he fa ps ur`) mirroring the whole layout, not just the text.

- Upstream is actively building this: `crates/ui-egui/src/i18n.rs` has English plus 10 languages (es, zh, ja, ko, ru, pt-BR, de, cs, fr, sr), a View-tab language switcher that defaults to the system language, and right-to-left sheets (`tests/right_to_left.rs`). `docs/localization-parity.md` estimates the catalog covers about 12% of about 2,200 user-visible strings. Extend upstream's catalog and switcher rather than building a parallel one, and expect rebase conflicts while upstream keeps adding strings.
- Missing pieces: complex-text shaping (Arabic joining, Devanagari reordering) and bidirectional UI layout; mirrored ribbon, dialogs and panes for RTL.
- The language stays overridable in the UI (upstream's switcher). After a change, offer a "go back" control written in the previous language, so someone who picks a language they cannot read can find their way back.
