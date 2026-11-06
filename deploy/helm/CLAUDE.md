# The Helm chart

Published as `oci://ghcr.io/kaas-rs/charts/kaas-ui` by the `chart` job in
`release.yml`, on a version tag and after the image. `cargo xtask chart` is the
acceptance, and `cargo xtask ci` runs it.

- **Versions move in lockstep.** One tag, one image, one chart, so `Chart.yaml`'s
  `version` and `appVersion` equal the workspace's, and the check fails otherwise.
  **A release commit bumps `Chart.yaml` too.**
- **Argo renders with Helm 4.** CI pins the version ArgoCD bundles, not the one
  on the dev box. `kubectl -n argocd exec deploy/argocd-repo-server -c
  argocd-repo-server -- helm version --short` says what that is.
- **Editing `environments` must not restart the pod.** kaas-ui swaps the fleet
  live and a restart signs everyone out, so `checksum/startup-config` hashes every
  block *but* that one. The ConfigMap is mounted as a directory because a `subPath`
  mount never sees an update.
- **`enableServiceLinks: false` is not a value.** A Service named `kaas-ui` injects
  `KAAS_UI_*` variables under the prefix the binary reads.
- **No `helm.sh/chart` on the pod template.** A chart change that alters nothing
  about the pod must not roll it.
- **The selector is immutable on a live Deployment.** Changing
  `kaas-ui.selectorLabels` means deleting every installed Deployment by hand.
- **The default values do not render, on purpose.** kaas-ui refuses an empty fleet
  and there is no sensible default cluster, so refusing at render beats a crash
  loop. This is why `helm lint` runs against `ci/full.yaml`.
- **Add a case to `ci/` rather than trusting a render.** `ci/*.yaml` must render,
  and its `config.yaml` goes through `Config::from_yaml`. `ci/*.invalid.yaml` must
  be refused for the reason on its first line, `# expect: <text>`.
- **Dex is not in the chart.** Any OIDC provider works, so it is the deployment's
  choice. In this cluster it is plain manifests beside the chart, in
  `k3s-cluster/apps/kaas-ui/`.
