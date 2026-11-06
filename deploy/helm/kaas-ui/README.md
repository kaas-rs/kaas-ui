# kaas-ui

A multi-cluster Kafka UI: topics, messages, consumer groups, schemas, ACLs and
Kafka Connect, with OIDC sign-in and per-role access. It is one small static
binary, and it never talks to the Kubernetes API.

```sh
helm install kaas-ui oci://ghcr.io/kaas-rs/charts/kaas-ui \
  --version <version> --values values.yaml
```

The chart and the image are published together from one tag, so a chart version
installs the image of the same version. Both are signed:

```sh
cosign verify ghcr.io/kaas-rs/charts/kaas-ui:<version> \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github\.com/kaas-rs/kaas-ui/'
```

## The minimum

`config` is kaas-ui's configuration file, verbatim. It needs at least one
environment, and rendering refuses without one, as the binary would at startup.

```yaml
config:
  environments:
    - id: dev
      name: Development
      kafka_clusters:
        - id: kafka
          name: kafka
          bootstrap: ["kafka-bootstrap.kafka.svc.cluster.local:9092"]
```

With no `roles`, this is an **open deployment**: everyone who can reach it is an
anonymous caller who can see every cluster and read every record, and nothing can
be written. Keep it behind something, or configure sign-in.

## Credentials

Never put a secret in `config`, because it lands in a ConfigMap. Omit the key and
kaas-ui reads `KAAS_UI_<CREDENTIAL>_<ENVIRONMENT>_<ID>` instead:

```yaml
extraEnv:
  - name: KAAS_UI_PASSWORD_DEV_KAFKA
    valueFrom:
      secretKeyRef:
        name: kafka-credentials
        key: password
config:
  environments:
    - id: dev
      name: Development
      kafka_clusters:
        - id: kafka
          name: kafka
          bootstrap: ["kafka-bootstrap.kafka.svc.cluster.local:9095"]
          sasl:
            mechanism: scram-sha-512
            username: kaas-ui
```

Credentials are read at startup, so rotating one needs a restart.

## TLS

PEM stays a file. Mount the CA beside `/etc/kaas-ui`, not inside it, and name the
path in the cluster's `tls` block:

```yaml
extraVolumes:
  - name: kafka-ca
    secret:
      secretName: kafka-ca
extraVolumeMounts:
  - name: kafka-ca
    mountPath: /etc/kafka-ca
    readOnly: true
```

```yaml
          tls:
            ca_file: /etc/kafka-ca/ca.crt
```

## Workload identity

`client_assertion_file` reads a signed assertion that something else keeps fresh,
so a deployment can hold no Kafka credential at all. Add whatever writes the file
with `initContainers`, where a native sidecar is an init container with
`restartPolicy: Always`, and mount the volume they share with `extraVolumes` and
`extraVolumeMounts`. State `serviceAccount.name`, since it is usually part of the
subject the provider trusts.

## Signing in

```yaml
config:
  auth:
    issuer: https://kaas-ui.example.com/dex
    client_id: kaas-ui
    redirect_url: https://kaas-ui.example.com/auth/callback
  roles:
    - name: admin
      subjects: ["someone@example.com"]
      clusters: ["*"]
      permissions:
        - resource: topic
          actions: [all]
```

Any OIDC provider works, and this chart does not install one. kaas-ui can also
serve a [Dex](https://dexidp.io) you run yourself under its own hostname: set
`config.dex.upstream` to its in-cluster address and `auth.issuer` to `/dex` on the
public URL.

Sessions are encrypted with a key generated per process, which has two
consequences. A restart signs everyone out, and `replicaCount` above 1 cannot work
with `auth`, because each pod would reject the others' cookies. Rendering refuses
that combination.

## Changing the configuration

| editing | what happens |
|---|---|
| `config.environments` | applied live within five seconds; the connections of unchanged clusters are kept, and the pod is not restarted |
| any other block of `config` | the pod rolls, because the rest is read once at startup, and everyone is signed out |

A bad edit to `environments` is logged, and the previous fleet stays in place.

## Exposing it

`service` is a ClusterIP on port 80, and `ingress` is available. Two things about
proxies:

- **Buffering breaks the live message view.** A proxy that buffers
  `text/event-stream` delivers records in stale bursts, so turn buffering off for
  this host.
- **Hosting under a path** needs `config.server.base_path` and a proxy that strips
  the prefix. The prefix has to be stated, because a stripping proxy leaves no
  record of what it removed.

## Values

`values.yaml` documents each key. `values.schema.json` closes the chart's own keys,
so a misspelt one fails the render rather than being ignored. `config` stays open
in the schema and is checked by the binary, which refuses unknown keys at startup.

`existingConfigMap` uses a ConfigMap you manage, with the file under the key
`config.yaml`. The chart cannot see inside it, so none of the checks above apply,
and a change to a startup-only block needs its own restart.
