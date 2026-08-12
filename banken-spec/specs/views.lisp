;; banken canonical views — the (defk8sview) authoring surface (BANKEN.md §III.b).
;;
;; These forms declare banken's navigable views as Lisp data and are
;; driven into typed K8sViewSpec values by banken_spec::load_views
;; (tatara_lisp::compile_typed under the (defk8sview) keyword). The kwargs
;; shapes below round-trip through the typed border by construction — a
;; `tests/lisp_roundtrip.rs` proves it.

;; NOTE on `:field object-name` — it is NOT a spelling preference.
;; `egaku::TableView` short-circuits the reserved field `name` to
;; `TableRow::identity()`, and banken's identity is the object UID (the `Grip`
;; seal: an act must address an object across delete-and-recreate). A NAME
;; column on the reserved field therefore draws a uid — measured 2026-08-09
;; against alpha-eks, 69 rows of `10a69bf6-b039-…`. `object-name` is an
;; ordinary field, so it projects through `Row::cell` and renders the name.
;; See `banken_spec::env::DISPLAY_NAME_FIELD`.

(defk8sview
  :name "pods"
  :kind ResourceTable
  :source (:resource pod)
  :columns ((:header "NAME" :field object-name)
            (:header "READY" :field ready)
            (:header "STATUS" :field phase)
            (:header "RESTARTS" :field restarts)
            (:header "AGE" :field age))
  :default-sort (:column "STATUS" :order desc)
  :drill-to "logs")

(defk8sview
  :name "svc"
  :kind ResourceTable
  :source (:resource service)
  :columns ((:header "NAME" :field object-name)
            (:header "TYPE" :field type)
            (:header "CLUSTER-IP" :field cluster-ip)
            (:header "AGE" :field age))
  :default-sort (:column "NAME" :order asc))

(defk8sview
  :name "ward"
  :kind HealthWard
  :source health
  :columns ((:header "WORKLOAD" :field object-name)
            (:header "MEM" :field mem-band)
            (:header "CPU" :field cpu-band)
            (:header "STATUS" :field phase))
  :default-sort (:column "STATUS" :order desc)
  :drill-to "diagnose")

;; ── The kinds the live backend gained when `list_resources` stopped being
;; pod-shaped (2026-08-12). Each is a view because the read exists; a kind
;; that reads live and has no `(defk8sview)` is navigable by nothing, which
;; `every_resource_kind_is_reachable_from_some_view` now refuses.
;;
;; Columns mirror `kubectl get <kind>` where the mapping is unambiguous — an
;; operator should not have to learn a second column vocabulary to read the
;; same object.
;;
;; `:resource` values are the SERDE variant names (`replica_set`, not
;; `replicaset`); a typo is a compile error naming the legal set, which is
;; how the first draft of this block was caught.

(defk8sview
  :name "deploy"
  :kind ResourceTable
  :source (:resource deployment)
  :columns ((:header "NAME" :field object-name)
            (:header "READY" :field ready)
            (:header "UP-TO-DATE" :field up-to-date)
            (:header "AVAILABLE" :field available)
            (:header "AGE" :field age))
  :default-sort (:column "NAME" :order asc))

(defk8sview
  :name "rs"
  :kind ResourceTable
  :source (:resource replica_set)
  :columns ((:header "NAME" :field object-name)
            (:header "DESIRED" :field desired)
            (:header "CURRENT" :field current)
            (:header "READY" :field ready)
            (:header "AGE" :field age))
  :default-sort (:column "NAME" :order asc))

(defk8sview
  :name "no"
  :kind ResourceTable
  :source (:resource node)
  :columns ((:header "NAME" :field object-name)
            (:header "STATUS" :field phase)
            (:header "VERSION" :field version)
            (:header "AGE" :field age))
  :default-sort (:column "STATUS" :order desc))

(defk8sview
  :name "ns"
  :kind ResourceTable
  :source (:resource namespace)
  :columns ((:header "NAME" :field object-name)
            (:header "STATUS" :field phase)
            (:header "AGE" :field age))
  :default-sort (:column "NAME" :order asc))

;; KEYS, not values — the projection deliberately carries a count. A
;; ConfigMap routinely holds a credential someone put there by mistake, and
;; rendering it into a terminal publishes it to a scrollback, a screen share
;; and a recording. See `configmap_to_row`.
(defk8sview
  :name "cm"
  :kind ResourceTable
  :source (:resource config_map)
  :columns ((:header "NAME" :field object-name)
            (:header "KEYS" :field keys)
            (:header "AGE" :field age))
  :default-sort (:column "NAME" :order asc))

(defk8sview
  :name "ep"
  :kind ResourceTable
  :source (:resource endpoints)
  :columns ((:header "NAME" :field object-name)
            (:header "ENDPOINTS" :field endpoints)
            (:header "AGE" :field age))
  :default-sort (:column "NAME" :order asc))

;; Events. A navigable view and not only a `ClusterEnv::events` call: this is
;; the first place an operator looks when a workload will not start, and a
;; read reachable from no view is reachable by nobody.
;;
;; COUNT is a column because k8s collapses repeats into one object with a
;; count rather than emitting each — an event that fired 400 times and one
;; that fired once are different situations wearing the same message.
(defk8sview
  :name "ev"
  :kind ResourceTable
  :source (:resource event)
  :columns ((:header "TYPE" :field event-type)
            (:header "REASON" :field reason)
            (:header "OBJECT" :field object)
            (:header "COUNT" :field count)
            (:header "MESSAGE" :field message)
            (:header "AGE" :field age))
  :default-sort (:column "AGE" :order desc))
