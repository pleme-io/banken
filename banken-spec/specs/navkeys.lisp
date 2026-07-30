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

;; Quit.
(defnavkey :name "quit"              :keys "q"      :intent quit)
