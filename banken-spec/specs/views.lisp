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
;; against camelot-eks, 69 rows of `10a69bf6-b039-…`. `object-name` is an
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
