;; banken canonical views — the (defk8sview) authoring surface (BANKEN.md §III.b).
;;
;; These forms declare banken's navigable views as Lisp data and are
;; driven into typed K8sViewSpec values by banken_spec::load_views
;; (tatara_lisp::compile_typed under the (defk8sview) keyword). The kwargs
;; shapes below round-trip through the typed border by construction — a
;; `tests/lisp_roundtrip.rs` proves it.

(defk8sview
  :name "pods"
  :kind ResourceTable
  :source (:resource pod)
  :columns ((:header "NAME" :field name)
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
  :columns ((:header "NAME" :field name)
            (:header "TYPE" :field type)
            (:header "CLUSTER-IP" :field cluster-ip)
            (:header "AGE" :field age))
  :default-sort (:column "NAME" :order asc))

(defk8sview
  :name "ward"
  :kind HealthWard
  :source health
  :columns ((:header "WORKLOAD" :field name)
            (:header "MEM" :field mem-band)
            (:header "CPU" :field cpu-band)
            (:header "STATUS" :field phase))
  :default-sort (:column "STATUS" :order desc)
  :drill-to "diagnose")
