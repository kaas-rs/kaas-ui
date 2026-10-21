// The writes a connector page can offer, and the one dialog they all go
// through.
//
// Phase 10, GitOps first. Three things decide what appears here, all on the
// server: the Connect cluster's `writes.allow` ceiling, the caller's roles,
// and whether Git owns this connector. The page draws a button only for a
// write every gate agrees to — a button that 403s is worse than none — and
// for a write refused because Git owns the connector it draws the change to
// make in Git instead. Every dialog shows that equivalent too, applied or not.

import { useState } from "react"
import { GitBranch, Pause, Play, RotateCw, Square } from "lucide-react"

import { ApiError, useConnectorWrite } from "@/api/client"
import type {
  ConnectorStatus,
  Equivalent,
  Operation,
  Owned,
  WriteOption,
} from "@/api/types"
import { Mono } from "@/components/domain"
import { Button } from "@/components/ui/button"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"

const LABEL: Record<Operation, string> = {
  "connector.restart": "restart",
  "connector.pause": "pause",
  "connector.resume": "resume",
  "connector.stop": "stop",
}

const ICON: Record<Operation, typeof RotateCw> = {
  "connector.restart": RotateCw,
  "connector.pause": Pause,
  "connector.resume": Play,
  "connector.stop": Square,
}

/** Whether an operation makes sense from this state — pausing a paused
    connector is not a button worth drawing. */
function fits(operation: Operation, status: ConnectorStatus): boolean {
  switch (operation) {
    case "connector.pause":
      return status !== "paused" && status !== "stopped"
    case "connector.resume":
      return status === "paused" || status === "stopped"
    case "connector.stop":
      return status !== "stopped"
    default:
      return true
  }
}

export function ConnectorWrites({
  envId,
  connectId,
  connector,
  status,
  writes,
  managedInGit,
}: {
  envId: string
  connectId: string
  connector: string
  status: ConnectorStatus
  writes: WriteOption[]
  managedInGit: Owned | null
}) {
  const [open, setOpen] = useState<WriteOption | "git" | null>(null)
  const allowed = writes.filter(
    (write) => write.allowed && fits(write.operation, status)
  )
  const owned = writes.filter(
    (write) => write.refusedBy === "owned" && fits(write.operation, status)
  )
  if (allowed.length === 0 && owned.length === 0 && !managedInGit) return null

  return (
    <div className="flex flex-wrap items-center gap-2">
      {managedInGit ? (
        <span
          className="inline-flex items-center gap-1 rounded-sm border px-2 py-0.5 text-[12px] text-ink-muted"
          title={managedInGit.source ?? "Git owns this connector"}
        >
          <GitBranch aria-hidden className="size-3.5" />
          managed in Git
        </span>
      ) : null}
      {allowed.map((write) => {
        const Icon = ICON[write.operation]
        return (
          <Button
            key={write.operation}
            variant="outline"
            size="sm"
            onClick={() => setOpen(write)}
          >
            <Icon aria-hidden />
            {LABEL[write.operation]}
          </Button>
        )
      })}
      {owned.length > 0 ? (
        <Button variant="ghost" size="sm" onClick={() => setOpen("git")}>
          <GitBranch aria-hidden />
          change in Git
        </Button>
      ) : null}

      <Sheet
        open={open !== null}
        onOpenChange={(next) => {
          if (!next) setOpen(null)
        }}
      >
        <SheetContent className="w-full sm:max-w-lg">
          {open === "git" ? (
            <GitChanges
              connector={connector}
              owned={owned}
              managedInGit={managedInGit}
            />
          ) : open ? (
            <WriteForm
              key={open.operation}
              envId={envId}
              connectId={connectId}
              connector={connector}
              write={open}
              onDone={() => setOpen(null)}
            />
          ) : null}
        </SheetContent>
      </Sheet>
    </div>
  )
}

function WriteForm({
  envId,
  connectId,
  connector,
  write,
  onDone,
}: {
  envId: string
  connectId: string
  connector: string
  write: WriteOption
  onDone: () => void
}) {
  const mutation = useConnectorWrite(envId, connectId, connector)
  // The safe defaults for a button: restart what has failed, tasks included.
  // On a healthy connector that restarts nothing, which is the right outcome
  // for a click nobody meant.
  const [includeTasks, setIncludeTasks] = useState(true)
  const [onlyFailed, setOnlyFailed] = useState(true)
  const restart = write.operation === "connector.restart"
  const failure = mutation.error as ApiError | null

  return (
    <>
      <SheetHeader>
        <SheetTitle>
          {LABEL[write.operation]}{" "}
          <span className="font-mono">{connector}</span>
        </SheetTitle>
        <SheetDescription>
          {write.tier === "operate"
            ? "Not declared state: nothing in Git changes, and it is allowed even where Git owns the connector."
            : "Changes the connector's declared state. Where Git owns a connector this is refused; here it is not."}
        </SheetDescription>
      </SheetHeader>

      <div className="space-y-4 px-4">
        {restart ? (
          <fieldset className="space-y-2 text-[13px]">
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={includeTasks}
                onChange={(event) => setIncludeTasks(event.target.checked)}
              />
              its tasks too
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={onlyFailed}
                onChange={(event) => setOnlyFailed(event.target.checked)}
              />
              only what has failed
            </label>
          </fieldset>
        ) : null}

        <EquivalentBlock equivalent={write.equivalent} />

        <p className="text-[12px] text-ink-faint">
          Recorded in the audit before it is sent, with your name and roles; if
          it cannot be recorded, it is not sent.
        </p>

        {mutation.isSuccess ? (
          <p className="text-[13px] text-ok">
            Done. Audit <Mono>{mutation.data.auditId}</Mono>.
          </p>
        ) : null}
        {failure ? (
          <div className="space-y-2">
            <p className="text-[13px] text-danger">{failure.message}</p>
            {failure.equivalent ? (
              <EquivalentBlock equivalent={failure.equivalent} />
            ) : null}
          </div>
        ) : null}
      </div>

      <SheetFooter>
        {mutation.isSuccess ? (
          <Button onClick={onDone}>close</Button>
        ) : (
          <Button
            variant={
              write.operation === "connector.stop" ? "destructive" : "default"
            }
            disabled={mutation.isPending}
            onClick={() =>
              mutation.mutate({
                operation: write.operation,
                request: restart ? { includeTasks, onlyFailed } : undefined,
              })
            }
          >
            {mutation.isPending ? "sending…" : LABEL[write.operation]}
          </Button>
        )}
      </SheetFooter>
    </>
  )
}

function GitChanges({
  connector,
  owned,
  managedInGit,
}: {
  connector: string
  owned: WriteOption[]
  managedInGit: Owned | null
}) {
  return (
    <>
      <SheetHeader>
        <SheetTitle>
          change <span className="font-mono">{connector}</span> in Git
        </SheetTitle>
        <SheetDescription>
          Git owns this connector, so its declared state is changed there — the
          operator would put back an edit made here.
          {managedInGit?.source ? (
            <>
              {" "}
              It lives in <Mono>{managedInGit.source}</Mono>.
            </>
          ) : null}
        </SheetDescription>
      </SheetHeader>
      <div className="space-y-5 px-4">
        {owned.map((write) => (
          <div key={write.operation} className="space-y-1.5">
            <h3 className="text-[13px] font-semibold">
              {LABEL[write.operation]}
            </h3>
            <EquivalentBlock equivalent={write.equivalent} />
          </div>
        ))}
      </div>
    </>
  )
}

function EquivalentBlock({ equivalent }: { equivalent: Equivalent }) {
  return (
    <div className="space-y-1">
      <pre className="overflow-auto rounded-sm border bg-muted/40 p-3 font-mono text-[12px] leading-relaxed">
        {equivalent.text}
      </pre>
      <p className="text-[12px] text-ink-faint">{equivalent.note}</p>
    </div>
  )
}
