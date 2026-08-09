;; banken navigation keys — the (defnavkey) authoring surface.
;;
;; These were HARDCODED in `banken/src/app.rs`'s `default_keymap()` while the
;; three postigo chords were authored in `actions.lisp` — a keymap half data,
;; half Rust, with the two halves sharing one keyboard and NO shared conflict
;; check. `Catalog::resolve` now conflict-checks both domains against one
;; chord namespace, so a nav key landing on a postigo chord is an error
;; instead of last-write-wins in whichever `km.bind` ran last.
;;
;; A nav key deliberately carries NO `postigo` legality class: it mutates
;; local UI state only. Typing "move the cursor" as `observe` would make the
;; legality class stop meaning "this performed a cluster read".
;;
;; Chords are typed through awase::Hotkey, so an unparseable chord has no
;; value. Note `escape` (awase's spelling) — egaku-term delivers the same key
;; as `esc`, and `banken::keys::chord_to_combo` carries that ONE verified
;; translation rather than guessing by Display equality.

;; k9s + vi both move the selection; two bindings, one intent. That is why
;; `:name` and `:intent` are separate fields.
(defnavkey :name "select-next-arrow" :keys "down"   :intent select-next)
(defnavkey :name "select-next-vi"    :keys "j"      :intent select-next)
(defnavkey :name "select-prev-arrow" :keys "up"     :intent select-prev)
(defnavkey :name "select-prev-vi"    :keys "k"      :intent select-prev)

;; Cycle the sort direction on the active column.
(defnavkey :name "toggle-sort"       :keys "o"      :intent toggle-sort)

;; Dismiss the postigo action-result overlay.
(defnavkey :name "dismiss-overlay"   :keys "escape" :intent dismiss)

;; Confirm the previewed action. Today that is exactly one thing: opening the
;; `(defbancada)` session the `g` / `shift+g` overlay is previewing. A
;; bancada is NEVER opened by the chord that resolves it — the operator sees
;; the fully-resolved argv, the cluster it names, and the DERIVED postigo
;; class first, and then decides. That gap is the whole point for a
;; BREAK-GLASS recipe. Note awase spells this `return`; egaku-term delivers
;; it as `enter`, and `banken::keys::chord_to_combo` carries that one
;; MEASURED translation.
(defnavkey :name "confirm-preview"   :keys "return" :intent confirm)

;; Help — the authored vocabulary, rendered back to the operator.
;;
;; TWO chords, one intent, the same shape as `down`/`j`: `h` is the one an
;; operator reaches for, `f1` is the one that works on any keyboard layout.
;;
;; `?` — the k9s/vim idiom, and what was authored first — is NOT here, and the
;; reason is measured rather than aesthetic. `awase 0.1.6` (what banken pins)
;; has no `?` in `Key::from_name`, so `(defnavkey :keys "?")` fails to compile
;; the catalog outright: `invalid hotkey: unknown key: ?`. Worse, egaku-term's
;; `to_hotkey` resolves a delivered `Char('?')` through that same function, so
;; even a version that PARSED `?` would be delivering it as `Key::Slash` with
;; no modifier — awase models one `Slash` key and cannot distinguish the
;; shifted glyph — making `?` and `/` literally the same chord and burning the
;; k9s filter key to buy the k9s help key.
;;
;; `pending-banken: help-question-chord` — `?` becomes authorable when awase
;; models a shifted glyph distinctly from its unshifted key. Until then the
;; status-line legend carries `h:help`, so the chord is discoverable rather
;; than merely documented.
(defnavkey :name "help"              :keys "h"      :intent help)
(defnavkey :name "help-f1"           :keys "f1"     :intent help)

;; Quit.
(defnavkey :name "quit"              :keys "q"      :intent quit)
