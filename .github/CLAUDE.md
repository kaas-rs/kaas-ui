# CI

CI runs on `arc-runner-set-ui`, a repo-scoped ARC scale set in the same k3s
cluster. Fork **and Dependabot** PRs are routed to GitHub-hosted runners — the
ARC pods are privileged DinD on a node hostPath and this repo is public, and a
Dependabot PR is unreviewed code from this repo's own branch, which the fork
test does not catch. `actionlint` runs before anything else, because a workflow
expression error fails in zero seconds with no logs.

Every action is pinned to a full commit SHA with its version as a comment.
A Dependabot PR is the one branch that lands on `main` by PR. Dependabot follows
`dtolnay/rust-toolchain`'s `# master` pin as a branch, but cannot see the
cargo-audit binary `ci.yml` downloads, or the Helm version passed to
`azure/setup-helm`, which follows the cluster's ArgoCD; bump those by hand.
