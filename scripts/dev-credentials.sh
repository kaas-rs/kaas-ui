#!/usr/bin/env bash
# Fetch what `config.dev.yaml` needs to reach the two development clusters
# into `.dev/`, which is gitignored.
#
# Neither cluster has an anonymous listener any more — both serve mtls, scram
# and oauth on 9093/9095/9096 and nothing on 9092. The deployment uses oauth
# by workload identity, which only works from the kaas-ui ServiceAccount, so a
# development run uses SCRAM on 9095 as the `kafbat-ui` KafkaUser instead: it
# exists on both clusters, and its ACLs cover every topic, group and the
# cluster resource, which is what a fleet view describes.
#
# Explicit on purpose: kaas-ui never goes looking for a secret, so this is the
# step that hands one over. It writes two CA bundles and an env file holding
# `KAAS_UI_PASSWORD_DEV_{KAAS,STRIMZI}`. The debug launch reads the env file
# itself; for a terminal run or `cargo xtask live`:
#
#   set -a; . .dev/credentials.env; set +a
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/.dev"
mkdir -p "$out"

secret() { kubectl get secret -n "$1" "$2" -o "jsonpath={.data.$3}" | base64 -d; }

# The CA each listener's certificate chains to. Hostname verification stays
# on: both certificates name the bootstrap Service dialled here.
secret kaas kaas-broker-internal-tls 'ca\.crt' >"$out/kaas-ca.crt"
secret strimzi kafka-cluster-cluster-ca-cert 'ca\.crt' >"$out/strimzi-ca.crt"

umask 077
{
  echo "KAAS_UI_PASSWORD_DEV_KAAS=$(secret kaas kafbat-ui-kafka-credentials password)"
  echo "KAAS_UI_PASSWORD_DEV_STRIMZI=$(secret strimzi kafbat-ui password)"
} >"$out/credentials.env.tmp"
mv "$out/credentials.env.tmp" "$out/credentials.env"

echo "dev credentials written to $out"
