import type { ReactNode } from "react"
import { formatDistanceStrict } from "date-fns"

import { useBrokerTls } from "@/api/client"
import { Mono, Section, Stat } from "@/components/domain"
import { Card, CardContent } from "@/components/ui/card"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import type { BrokerTls, Certificate, ResourceError } from "@/api/types"
import { ExpandLink } from "./expand-link"

/**
 * Inside this window an expiry is worth a colour. Strimzi renews its CA thirty
 * days out by default, so a leaf this close that has *not* rotated is a
 * renewal that did not happen.
 */
const SOON_MS = 30 * 24 * 60 * 60 * 1000

type Expiry = "expired" | "soon" | "ok"

function expiry(cert: Certificate, now: number): Expiry {
  if (cert.notAfter <= now) return "expired"
  if (cert.notAfter - now <= SOON_MS) return "soon"
  return "ok"
}

const EXPIRY_CLASS: Record<Expiry, string> = {
  expired: "font-medium text-danger",
  soon: "font-medium text-warn-ink",
  ok: "text-ink-muted",
}

function relative(at: number, now: number): string {
  const distance = formatDistanceStrict(at, now)
  return at <= now ? `expired ${distance} ago` : `expires in ${distance}`
}

function utc(at: number): string {
  // UTC rather than the reader's zone: it is what `openssl x509 -enddate`
  // prints, so the two compare by eye.
  return `${new Date(at).toISOString().slice(0, 16).replace("T", " ")} UTC`
}

/** The chain's earliest expiry, which is the one that will break first. */
function earliest(chain: Certificate[]): Certificate | undefined {
  return chain.reduce<Certificate | undefined>(
    (first, cert) =>
      first === undefined || cert.notAfter < first.notAfter ? cert : first,
    undefined
  )
}

/** A broker's error, found by the resource name the server gave it. */
export function brokerError(
  errors: ResourceError[],
  nodeId: number
): ResourceError | undefined {
  return errors.find((error) => error.resource === `broker ${nodeId}`)
}

/**
 * The certificate cell of one broker row: the leaf's expiry, coloured by how
 * close it is, and the toggle for the chain below the table.
 */
export function TlsCell({
  tls,
  error,
  loading,
  open,
  onToggle,
}: {
  tls: BrokerTls | undefined
  error: ResourceError | undefined
  loading: boolean
  open: boolean
  onToggle: () => void
}) {
  if (loading) return <span className="text-[12px] text-ink-faint">…</span>
  if (error) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <ExpandLink open={open} onToggle={onToggle} className="text-danger">
            {error.kind === "decode" ? "unreadable" : "no session"}
          </ExpandLink>
        </TooltipTrigger>
        <TooltipContent>{error.message}</TooltipContent>
      </Tooltip>
    )
  }
  if (!tls) return <span className="text-ink-faint">—</span>
  if (tls.session === null) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="text-[12px] text-ink-faint">plaintext</span>
        </TooltipTrigger>
        <TooltipContent>
          the listener kaas-ui dials is not encrypted — a configuration, not a
          failure
        </TooltipContent>
      </Tooltip>
    )
  }
  const now = Date.now()
  const leaf = tls.session.chain[0]
  const first = earliest(tls.session.chain)
  if (!leaf || !first) return <span className="text-ink-faint">—</span>
  const state = expiry(first, now)
  return (
    <ExpandLink
      open={open}
      onToggle={onToggle}
      className={EXPIRY_CLASS[state]}
      title={first === leaf ? "the leaf" : `the chain's ${first.subject}`}
    >
      {relative(first.notAfter, now)}
    </ExpandLink>
  )
}

/** The CN of a distinguished name, which is what an operator calls a CA. */
function commonName(dn: string): string {
  return /(?:^|,)CN=([^,]+)/.exec(dn)?.[1] ?? dn
}

/**
 * The cluster's certificates folded into the same stat grid as the counts
 * above it: protocol, the earliest expiry and which broker holds it, and
 * whether every leaf shares an issuer. A summary of the broker rows, not a
 * cluster certificate — there is no such thing.
 */
function TlsSummary({
  items,
  errors,
}: {
  items: BrokerTls[]
  errors: ResourceError[]
}) {
  const sessions = items.flatMap((item) =>
    item.session ? [{ nodeId: item.nodeId, session: item.session }] : []
  )
  const failed = errors.filter((error) => error.resource.startsWith("broker "))
  if (items.length === 0 && failed.length === 0) return null

  const now = Date.now()
  const plaintext = items.length - sessions.length
  const protocols = [...new Set(sessions.map((s) => s.session.protocolVersion))]
  const issuers = [
    ...new Set(sessions.flatMap((s) => s.session.chain[0]?.issuer ?? [])),
  ]
  const soonest = sessions
    .flatMap((s) => {
      const first = earliest(s.session.chain)
      return first ? [{ nodeId: s.nodeId, cert: first }] : []
    })
    .sort((a, b) => a.cert.notAfter - b.cert.notAfter)[0]
  const state = soonest ? expiry(soonest.cert, now) : "ok"

  return (
    <dl className="grid grid-cols-3 gap-x-4 gap-y-2 text-[13px]">
      <Stat
        label="tls"
        value={
          sessions.length > 0
            ? protocols.join(", ")
            : failed.length > 0
              ? "—"
              : "plaintext"
        }
        note={
          plaintext > 0 && sessions.length > 0
            ? `${plaintext} plaintext`
            : undefined
        }
        hint="what kaas-ui's own connections negotiated, on the listener it dials — other listeners may present other certificates."
      />
      {soonest ? (
        <Stat
          label="earliest expiry"
          value={
            state === "expired"
              ? "expired"
              : formatDistanceStrict(soonest.cert.notAfter, now)
          }
          note={`broker ${soonest.nodeId}`}
          tone={
            state === "expired"
              ? "danger"
              : state === "soon"
                ? "warn"
                : undefined
          }
          hint="the first certificate in any broker's chain to run out — amber within 30 days."
        />
      ) : null}
      {issuers.length > 0 ? (
        <Stat
          label="issuer"
          value={
            issuers.length === 1
              ? commonName(issuers[0] ?? "")
              : `${issuers.length} issuers`
          }
          tone={issuers.length > 1 ? "warn" : undefined}
          hint="the CA that signed the brokers' leaves. More than one is a rotation in progress, or one that stalled."
        />
      ) : null}
      {failed.length > 0 ? (
        <Stat
          label="no session"
          value={String(failed.length)}
          tone="danger"
          hint="brokers whose handshake failed — the broker table says why."
        />
      ) : null}
    </dl>
  )
}

/**
 * The cluster overview's TLS card.
 *
 * Its own card rather than a row of the cluster's: these are facts about
 * kaas-ui's connections to each broker, not about the cluster's metadata. It
 * reads the broker table's query, so it costs no request, and it renders
 * nothing until there is something to say — no placeholder to flicker.
 */
export function TlsCard({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const tls = useBrokerTls(envId, clusterId)
  if (tls.error) {
    return (
      <Section title="TLS">
        <Card>
          <CardContent className="text-[13px] text-danger">
            {String(tls.error)}
          </CardContent>
        </Card>
      </Section>
    )
  }
  if (!tls.data) return null
  const { items, errors } = tls.data
  if (items.length === 0 && errors.length === 0) return null

  return (
    <Section title="TLS">
      <Card>
        <CardContent>
          <TlsSummary items={items} errors={errors} />
        </CardContent>
      </Card>
    </Section>
  )
}

/** One broker's chain, or the reason it has none. */
export function TlsDetail({
  nodeId,
  tls,
  error,
}: {
  nodeId: number
  tls: BrokerTls | undefined
  error: ResourceError | undefined
}) {
  if (error) {
    return (
      <Card className="p-4 text-[13px]">
        <p className="mb-1 font-medium text-danger">
          broker {nodeId}: no TLS session
        </p>
        <Mono>{error.message}</Mono>
        <p className="mt-2 text-[12px] text-ink-muted">
          A chain that failed verification produces no connection, so there is
          no certificate to show — the message above is what rustls rejected.
        </p>
      </Card>
    )
  }
  if (!tls?.session) return null
  const { session } = tls
  const now = Date.now()

  return (
    <div className="space-y-3">
      {session.chain.map((cert, index) => (
        <CertificateCard
          key={cert.sha256Fingerprint}
          cert={cert}
          role={index === 0 ? "leaf" : cert.selfSigned ? "root" : "issuer"}
          serverName={index === 0 ? session.serverName : null}
          now={now}
        />
      ))}
    </div>
  )
}

function CertificateCard({
  cert,
  role,
  serverName,
  now,
}: {
  cert: Certificate
  role: "leaf" | "issuer" | "root"
  serverName: string | null
  now: number
}) {
  const state = expiry(cert, now)
  const names = [...cert.dnsNames].sort()
  const leafLacksServerAuth =
    role === "leaf" &&
    cert.extendedKeyUsage.length > 0 &&
    !cert.extendedKeyUsage.includes("serverAuth")

  return (
    <Card className="p-4">
      <div className="mb-3 flex items-baseline gap-3">
        <span className="text-[11px] tracking-wide text-ink-faint uppercase">
          {role}
        </span>
        <span className="font-mono text-[13px]">{cert.subject}</span>
      </div>
      <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1.5 text-[12px]">
        <Field label="issuer">
          <Mono>{cert.issuer}</Mono>
        </Field>
        <Field label="valid">
          <span className="font-mono">
            {utc(cert.notBefore)} → {utc(cert.notAfter)}
          </span>{" "}
          <span className={EXPIRY_CLASS[state]}>
            ({relative(cert.notAfter, now)})
          </span>
        </Field>
        {names.length > 0 || cert.ipAddresses.length > 0 ? (
          <Field label="SANs">
            <div className="flex flex-wrap gap-x-3 gap-y-0.5 font-mono">
              {names.map((name) => (
                <span
                  key={name}
                  className={
                    name === serverName
                      ? "font-medium text-ok"
                      : "text-ink-muted"
                  }
                  title={
                    name === serverName
                      ? "the name this connection verified"
                      : undefined
                  }
                >
                  {name}
                </span>
              ))}
              {cert.ipAddresses.map((ip) => (
                <span key={ip} className="text-ink-muted">
                  IP:{ip}
                </span>
              ))}
            </div>
          </Field>
        ) : role === "leaf" ? (
          <Field label="SANs">
            <span className="text-warn-ink">
              none — clients that ignore the CN, rustls among them, cannot
              verify this name
            </span>
          </Field>
        ) : null}
        <Field label="key usage">
          {cert.keyUsage.length > 0 ? (
            <Mono>{cert.keyUsage.join(", ")}</Mono>
          ) : (
            <span className="text-ink-faint">not restricted</span>
          )}
        </Field>
        <Field label="extended">
          {cert.extendedKeyUsage.length > 0 ? (
            <span className={leafLacksServerAuth ? "text-danger" : undefined}>
              <Mono>{cert.extendedKeyUsage.join(", ")}</Mono>
              {leafLacksServerAuth ? " — no serverAuth" : null}
            </span>
          ) : (
            <span className="text-ink-faint">not restricted (any purpose)</span>
          )}
        </Field>
        <Field label="CA">
          {cert.isCa === null ? (
            <span className="text-ink-faint">no basicConstraints</span>
          ) : (
            <Mono>{cert.isCa ? "yes" : "no"}</Mono>
          )}
        </Field>
        <Field label="key">
          <Mono>
            {cert.publicKey} · {cert.signatureAlgorithm}
          </Mono>
        </Field>
        <Field label="serial">
          <Mono>{cert.serial}</Mono>
        </Field>
        <Field label="sha-256">
          <span className="font-mono text-[11px] break-all text-ink-muted">
            {cert.sha256Fingerprint}
          </span>
        </Field>
      </dl>
    </Card>
  )
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-ink-faint">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </>
  )
}
