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

;; The tree. banken's screens are a path, the same shape as the Kubernetes API
;; path: cluster / kind / namespace / object / detail. `descend` goes one level
;; down and `ascend` one level up. `ascend` at the top lands on the cluster
;; chooser. It never quits: quitting stays on `q`, `:q` and ctrl+c.
;;
;; `descend` on a session preview confirms it. A bancada is NEVER opened by the
;; chord that resolves it: the operator sees the resolved argv, the cluster and
;; the DERIVED postigo class first, and then presses enter.
(defnavkey :name "descend-enter"     :keys "return" :intent descend)
(defnavkey :name "descend-vi"        :keys "l"      :intent descend)
(defnavkey :name "descend-arrow"     :keys "right"  :intent descend)
(defnavkey :name "ascend-escape"     :keys "escape" :intent ascend)
(defnavkey :name "ascend-vi"         :keys "h"      :intent ascend)
(defnavkey :name "ascend-arrow"      :keys "left"   :intent ascend)

;; vim motions over the rows. `top` is a sequence: `g` twice.
(defnavkey :name "top-vi"            :keys "g"        :intent top)
(defnavkey :name "bottom-vi"         :keys "shift+g"  :intent bottom)
(defnavkey :name "page-down-vi"      :keys "ctrl+d"   :intent page-down)
(defnavkey :name "page-up-vi"        :keys "ctrl+u"   :intent page-up)
(defnavkey :name "page-down-key"     :keys "pagedown" :intent page-down)
(defnavkey :name "page-up-key"       :keys "pageup"   :intent page-up)

;; History: ctrl+o goes back, tab goes forward. A terminal sends ctrl+i as tab,
;; so `tab` IS vim's ctrl+i.
(defnavkey :name "back-vi"           :keys "ctrl+o" :intent back)
(defnavkey :name "forward-vi"        :keys "tab"    :intent forward)

;; The `/` filter. `:` stays structural: awase names no shifted glyph, so `:`
;; has no chord (the same reason `?` cannot be the help key).
(defnavkey :name "filter"            :keys "slash"  :intent filter)

;; Help. `h` ascends now, so help moved to `shift+h` beside `f1`.
(defnavkey :name "help"              :keys "shift+h" :intent help)
(defnavkey :name "help-f1"           :keys "f1"      :intent help)

;; Quit.
(defnavkey :name "quit"              :keys "q"      :intent quit)
