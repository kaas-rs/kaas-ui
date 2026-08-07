# Release and deployment

Not a phase — a track that runs alongside all of them. Its first deliverables
landed with [Phase 0](11-built.md) (an image that builds, CI that runs) and it
is finished only when Phase 4's secrets are wired.

Three things to set up, in this order, because each depends on the one before:

1. **CI on the ARC runners** — needs a new scale set in the cluster repo
2. **Release to GHCR** under `kaas-rs`
3. **GitOps deployment** via the ArgoCD ApplicationSet that already exists

---

## 1. CI on ARC runners

Actions run on `arc-runner-set-ui`, an Actions Runner Controller scale set on
the same k3s cluster kaas-ui talks to.

### A third scale set is required

`apps/arc-runners/kustomization.yaml` in the cluster repo currently defines
**two** scale sets, and they are deliberately repo-scoped rather than
org-scoped:

> The GitHub App (installation 150341189) has repo-level runner admin only;
> registering at the org needs `POST /orgs/{org}/actions/runners/registration-token`,
> which returns 403 for this installation. Commit 510bed4 tried exactly that and
> was reverted in e4b8325.

So kaas-ui cannot reuse either. It needs a third entry, `arc-runner-set-ui`,
pointed at `https://github.com/kaas-rs/kaas-ui`:

```yaml
  - name: gha-runner-scale-set
    repo: oci://ghcr.io/actions/actions-runner-controller-charts
    releaseName: arc-runner-set-ui
    namespace: arc-runners
    version: 0.14.1
    includeCRDs: false            # the first entry already emits them
    valuesMerge: replace
    valuesInline:
      githubConfigUrl: https://github.com/kaas-rs/kaas-ui
      githubConfigSecret: arc-github-app
      minRunners: 0
      maxRunners: 2
      containerMode:
        type: dind
      controllerServiceAccount:
        namespace: arc-systems
        name: arc-gha-rs-controller
```

`maxRunners: 2` — single-node cluster already hosting two other scale sets, two
Kafka clusters and everything else. kaas-ui's CI is Rust plus a Vite build, both
CPU-bound; two concurrent runners is the sensible ceiling.

**The scale set name is the runner label.** `runs-on: arc-runner-set-ui`.

### Cache patch

`arc-runner-set-lib` gets a hostPath `CARGO_HOME` at `/var/cache/arc-rust`
because kaas-lib's dependency tree is large and re-downloading it per job costs
minutes on a home connection. kaas-ui's tree is larger still — the same tree
plus axum, and then a `node_modules`.

So kaas-ui gets the same treatment on its **own** hostPath, plus npm:

```
/var/cache/arc-ui/cargo   → CARGO_HOME
/var/cache/arc-ui/npm     → npm_config_cache
```

Separate from `/var/cache/arc-rust` on purpose: two scale sets sharing one
cargo registry directory is fine in principle — cargo takes an advisory lock on
`$CARGO_HOME/registry/.package-cache` — but a corrupt cache would then take
both repositories' CI down at once, and disk is cheaper than that.

Same JSON 6902 shape as the existing patches, and for the same reason:
`AutoscalingRunnerSet.spec.template` is a `runtime.RawExtension`, so kustomize
has no schema to merge against and a strategic patch clobbers the whole
template.

### Fork PRs must not land on ARC

The runner pods are privileged DinD with a hostPath mount onto the node, and
kaas-ui is public. A fork pull request is arbitrary code from a stranger.

Every job reachable from `pull_request` picks its runner per event, exactly as
kaas-lib does:

```yaml
runs-on: ${{ github.event.pull_request.head.repo.fork && 'ubuntu-latest' || 'arc-runner-set-ui' }}
```

`github.event.pull_request` is null for push and dispatch, so those fall through
to ARC.

### What the ARC image does not have

Verified against the running image, and every one of these has already cost
kaas-lib a red build:

- **No `cc`, `gcc`, `ld`, `make` or `pkg-config`.** `ring` needs a C toolchain.
  `curl`, `unzip`, `tar` and `git` *are* present, and uid 1001 has passwordless
  sudo. Every Rust job starts with the same guarded `apt-get install gcc
  libc6-dev pkg-config` step kaas-lib uses.
- **musl needs more.** The release build targets
  `x86_64-unknown-linux-musl`, so `musl-tools` too.
- **The DinD sidecar is not ready when the runner container starts.** Any job
  running `docker` waits for it in a loop first.

### `actionlint` runs before anything else

kaas-lib learned this the expensive way: a workflow expression error is not a
runtime failure. GitHub creates the run, fails it in zero seconds with no jobs
and no logs, and `--log-failed` reports only "log not found". `release.yml`
carried one from the day it was written and every push produced a red run whose
publish path had never once executed.

It costs two seconds. It goes first, in every workflow.

---

## 2. Release to GHCR

**`ghcr.io/kaas-rs/kaas-ui`**, alongside `ghcr.io/kaas-rs/charts` where the
kaas broker's Helm chart already lives.

### The image

One image, built in three stages:

```dockerfile
# 1. frontend
FROM node:24-alpine AS web
# npm ci && npm run build   → web/dist

# 2. backend, static musl — no C toolchain at runtime, no librdkafka
FROM rust:1.97.1-alpine AS build
# the web/dist from stage 1 is embedded by rust-embed at compile time
# cargo build --release --target x86_64-unknown-linux-musl

# 3. distroless
FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /out/kaas-ui /kaas-ui
USER 65532:65532
ENTRYPOINT ["/kaas-ui"]
```

Order matters: `rust-embed` pulls `web/dist` in at compile time, so the frontend
stage must precede the Rust stage. Getting it backwards produces an image that
builds and serves 404s.

PLAN.md §10 predicts ~25 MB and ~15 MB RSS at idle, against the JVM original's
several hundred. **Measure both in the release job and write them into the job
summary.** The number is the claim; an unmeasured claim is marketing.

`rustls` with `ring`, no `aws-lc-sys`, no cmake — the same reason kaas-lib picks
`ring`, and it is what makes the static musl build a two-line Dockerfile rather
than a project.

### Tags

| trigger | tags |
|---|---|
| push to `main` | `main`, `sha-<short>` |
| tag `v1.2.3` | `1.2.3`, `1.2`, `1`, `latest` |

> On a `0.x` tag there is no `:0`, because `0` would mean "any 0.x" and 0.x
> makes no compatibility promise. So `v0.1.2` publishes `0.1.2`, `0.1` and
> `latest`.
>
> **That is something `release.yml` does, not something the action does.**
> This paragraph used to claim `docker/metadata-action` suppressed the tag
> by itself; it does not, and every release from v0.4.0 through v0.8.1
> published a bare `0` while the claim sat here unchallenged. The action's
> README has a "Major version zero" section recommending the tag *should not*
> be generated and supplying the `enable=` expression that stops it — a
> recommendation misread as a behaviour. The guard is now on the
> `{{major}}` line and lapses by itself at `v1.0.0`.
>
> Nobody noticed for ten releases because the published tag list is only
> visible in the release log: this repo's `gh` token has no `read:packages`
> scope, so `gh api …/packages/container/kaas-ui/versions` returns 403 and
> the tags have to be read out of `DOCKER_METADATA_OUTPUT_JSON`.

**Bump `[workspace.package] version` in the same commit as the tag.** The
binary reports `CARGO_PKG_VERSION` from `GET /health`, which exists so a
running pod can be identified without exec'ing into it — and that only works
if the number in the manifest, the git tag and the image tag are the same
number. v0.1.1 shipped without the bump and the pod cheerfully reported
`0.1.0`, which is exactly the drift the endpoint is meant to expose.

`latest` only ever moves on a semver tag. An ArgoCD deployment pinned to
`latest` and a `main` build that clobbers it is how a cluster ends up running a
commit nobody released.

Multi-arch is deliberately **not** done initially: the cluster is a single amd64
node, and cross-building arm64 under QEMU on the same node would triple the
release time for nothing. Add `linux/arm64` when something needs it.

### Provenance

`packages: write` and `id-token: write`, push with `docker/build-push-action`,
and sign with `cosign` keylessly against the GitHub OIDC issuer. The image is
public, so no pull secret is needed in the cluster — same as
`oci://ghcr.io/kaas-rs/charts`.

### A chart, later

kaas publishes its Helm chart to `oci://ghcr.io/kaas-rs/charts`. kaas-ui should
too, **once the config surface has stopped moving** — around Phase 4, when auth
lands and the config file stops gaining a section per phase. Until then the
GitOps deployment below uses plain kustomize manifests, which is less work to
change and easier to read in a diff.

---

## 3. GitOps deployment

The cluster repo is `Woestebanaan/k3s-cluster`, checked out at
`/home/coder/repos/k3s-cluster`.

### It registers itself; it does not sync itself

`argocd-apps/appset-production.yaml` is an ApplicationSet with a git directory
generator over `apps/*`. **Creating `apps/kaas-ui/` and pushing registers the
Application** — there is no Application to write, no registration step. The
namespace is the directory basename and is created automatically
(`CreateNamespace=true`).

**Sync is manual.** The template sets `syncOptions` but no `automated:` block,
and no app in this cluster except `app-of-apps` has auto-sync enabled — several
(`apicurio`, `kaas`, `media`, `spire`, `strimzi`, `trivy-operator`) sit
`OutOfSync` as their steady state. So a push makes ArgoCD *notice* the change;
applying it is a deliberate act:

```sh
argocd app sync kaas-ui
# or: kubectl -n argocd patch application kaas-ui --type merge \
#       -p '{"operation":{"sync":{"revision":"HEAD"}}}'
```

This is worth knowing in both directions. It means a bad manifest cannot deploy
itself — but it also means a green CI run and a merged commit are **not**
evidence that anything changed in the cluster. Check `argocd app get kaas-ui`,
not the git log.

One edit is needed outside the new directory. The template assigns the ArgoCD
project by name:

```gotemplate
project: '{{ if or (eq .path.basename "microcks") (eq .path.basename "strimzi") (eq .path.basename "kaas") }}eventing{{ else ... }}platform{{ end }}'
```

kaas-ui belongs with the eventing set, so `"kaas-ui"` is added to that first
`or`. Without it the app lands in `platform`, which works but files it under the
wrong project.

> **Still hold the manifests until the first image exists.** Manual sync means
> a premature commit will not deploy a broken `Deployment` by itself — but it
> will leave a permanently `OutOfSync` app in a cluster that already has six of
> them, which is exactly how a real drift stops being noticeable.

### What `apps/kaas-ui/` contains

```
apps/kaas-ui/
  kustomization.yaml
  namespace.yaml
  configmap.yaml        the cluster registry — both Kafka clusters
  deployment.yaml
  service.yaml
```

**No `external-secret.yaml`, and none is coming.** This sketch assumed a
confidential OIDC client; kaas-ui shipped as a **public** client that proves
itself with PKCE, so there is no kaas-ui secret to mount from Vault. Dex has
one — its GitHub connector's client id and secret, in `apps/dex/` — and that is
the only ExternalSecret this login flow needs.

Plain kustomize, following `apps/code-server/` — the closest existing analogue.
Not Helm: there is no chart yet, and a chart's indirection is only worth paying
for once something else consumes it.

**Ingress** is Gateway API, not `Ingress`. The cluster runs
`traefik-gateway` in `kube-system` (address 192.168.1.60), and `apps/homepage`
and `apps/argocd` both attach `HTTPRoute`s to it. Hostname
`kaas-ui.smeding.cloud`, matching the existing `smeding.cloud` convention.

Note the ApplicationSet already carries `ignoreDifferences` for HTTPRoute
`parentRefs` group/kind and `backendRefs` group/kind/weight — Traefik defaults
those fields server-side and ArgoCD would otherwise show the app permanently
OutOfSync.

**Config** is a ConfigMap holding the same `config.yaml` documented in
[reference/environment.md](reference/environment.md), pointed at
`kaas.kaas.svc.cluster.local:9092` and
`kafka-cluster-kafka-bootstrap.strimzi.svc.cluster.local:9092`. The `dead`
cluster is a development fixture and does not belong in the deployed config.

**Resources.** `requests: 64Mi/50m`, `limits: 256Mi` and no CPU limit. The whole
point of the Rust rewrite is that this is a small process; requesting like a JVM
would forfeit the argument. Revisit only with numbers from the running pod.

**Security context.** `runAsNonRoot`, uid 65532, read-only root filesystem, all
capabilities dropped. Distroless nonroot already gives most of it; declaring it
means a base image change cannot quietly take it away.

**Probes.** `/health` for both liveness and readiness. It must never consult a
Kafka cluster — a liveness probe that fails because someone's broker is down is
a liveness probe that restarts a healthy process. This is a property of the
handler, asserted in Phase 0.

### Bumping the image tag

There is no ArgoCD Image Updater in this cluster, so the tag in
`apps/kaas-ui/kustomization.yaml` is bumped by a commit. The release workflow
prints the exact one-line change into its job summary so the bump is a copy, not
a lookup:

```yaml
images:
  - name: ghcr.io/kaas-rs/kaas-ui
    newTag: 0.1.0
```

Automating it means giving the kaas-ui repo a token that can push to the cluster
repo. Worth doing eventually; not worth doing before there is a release cadence
to automate.

---

## Deliverables by phase

| phase | release/deploy work |
|---|---|
| 0 | scale set added; `ci.yml` green on ARC; Dockerfile; `release.yml` pushing to GHCR; `apps/kaas-ui/` written but **not committed** |
| 0 → 1 | first `v0.1.0` tag; image published; `apps/kaas-ui/` committed; app live at `kaas.smeding.cloud` |
| 1–3 | tag bumps only |
| 4 | **done** — `apps/dex/` deployed with its GitHub connector's ExternalSecret, `apps/kaas-ui/` pointed at it and carrying `roles:`. No secret of kaas-ui's own: it is a public client |
| 4+ | consider publishing a chart to `oci://ghcr.io/kaas-rs/charts` |

## Acceptance

- `actionlint` passes on every workflow before anything else runs;
- a push to `main` runs `cargo xtask ci` on `arc-runner-set-ui` and goes green;
- a fork PR runs the same job on `ubuntu-latest` and never schedules onto ARC —
  verified by reading the job's runner label, not by assuming;
- a `v*` tag publishes `ghcr.io/kaas-rs/kaas-ui:<version>` and `:latest`,
  cosign-signed, with image size and idle RSS in the job summary;
- `kubectl -n kaas-ui get deploy` shows 1/1 and ArgoCD reports Synced/Healthy;
- `https://kaas.smeding.cloud/health` returns 200 — which needs the DNS record as
  well as the tunnel rule;
- the fleet page in the browser shows both live clusters — which is also the
  end-to-end proof that the deployed binary reaches them.
