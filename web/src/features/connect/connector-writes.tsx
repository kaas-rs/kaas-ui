// The writes a connector can take, and the one sheet they all go through.
//
// Phase 10, GitOps first. Three things decide what appears here, all on the
// server: the Connect cluster's `writes.allow` ceiling, the caller's roles,
// and whether Git owns this connector — and which of its aspects (config,
// existence, state, offsets) Git declares. A button is drawn only for a write
// every gate agrees to — a button that 403s is worse than none — and for a
// write refused because Git declares what it changes the page draws the
// change to make in Git instead, in whatever format this Connect cluster's
// Git holds connectors. Every sheet shows that equivalent too, applied or
// not.
//
// The tiers decide the friction, never the permission: operate is one click
// and a confirm, edit shows what will change, destroy wants the connector's
// name typed.

import { useState } from "react"
import { useNavigate } from "@tanstack/react-router"
import {
  ChevronDown,
  GitBranch,
  Pause,
  Pencil,
  Play,
  Plus,
  RotateCw,
  Square,
  Trash2,
  Undo2,
} from "lucide-react"

import {
  ApiError,
  useConnectPlugins,
  useConnectorCreate,
  useConnectorWrite,
} from "@/api/client"
import type {
  ConnectorStatus,
  Equivalent,
  Operation,
  Owned,
  RedactedConfig,
  StoredOffset,
  Validation,
  WriteOption,
  WriteResult,
} from "@/api/types"
import { Mono } from "@/components/domain"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
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
  "connector.restart_task": "restart task",
  "connector.pause": "pause",
  "connector.resume": "resume",
  "connector.stop": "stop",
  "connector.reset_topics": "reset active topics",
  "connector.edit": "edit config",
  "connector.create": "new connector",
  "connector.delete": "delete",
  "connector.reset_offsets": "reset offsets",
  "connector.alter_offsets": "alter offsets",
}

const ICON: Record<Operation, typeof RotateCw> = {
  "connector.restart": RotateCw,
  "connector.restart_task": RotateCw,
  "connector.pause": Pause,
  "connector.resume": Play,
  "connector.stop": Square,
  "connector.reset_topics": Undo2,
  "connector.edit": Pencil,
  "connector.create": Plus,
  "connector.delete": Trash2,
  "connector.reset_offsets": Undo2,
  "connector.alter_offsets": Pencil,
}

/** What each operation does, in one sentence, for the sheet's description. */
const WHAT: Record<Operation, string> = {
  "connector.restart":
    "Restarts the connector — and, as chosen, its tasks or only what failed.",
  "connector.restart_task": "Restarts one task. The others keep running.",
  "connector.pause":
    "Tasks keep their resources and process nothing until resumed.",
  "connector.resume": "Picks up where a paused or stopped connector left off.",
  "connector.stop":
    "Shuts the tasks down and releases their resources. Resuming takes longer than from a pause.",
  "connector.reset_topics":
    "Forgets which topics the connector has been seen using. The topics are untouched.",
  "connector.edit":
    "Replaces the whole config. The connector restarts its tasks with it.",
  "connector.create": "Creates a connector on this Connect cluster.",
  "connector.delete":
    "Deletes the connector and its tasks. Its offsets stay in the offsets topic.",
  "connector.reset_offsets":
    "Clears the stored position: a source starts over, a sink's group is reset. The connector must be stopped.",
  "connector.alter_offsets":
    "Sets the stored position partition by partition. The connector must be stopped.",
}

/** The operations on the header itself; the rest go under "more". */
const PRIMARY: Operation[] = [
  "connector.restart",
  "connector.pause",
  "connector.resume",
  "connector.stop",
  "connector.edit",
]

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
    case "connector.restart_task":
    case "connector.create":
      return false
    default:
      return true
  }
}

/** What the sheet needs to know about the connector it is changing. */
interface Subject {
  envId: string
  connectId: string
  connector: string
  status: ConnectorStatus
  config: RedactedConfig
  offsets: StoredOffset[] | null
}

type Open =
  { kind: "write"; write: WriteOption; task?: number } | { kind: "git" } | null

export function ConnectorWrites({
  writes,
  managedInGit,
  ...subject
}: Subject & {
  writes: WriteOption[]
  managedInGit: Owned | null
}) {
  const [open, setOpen] = useState<Open>(null)
  const usable = writes.filter((write) => fits(write.operation, subject.status))
  const allowed = usable.filter((write) => write.allowed)
  const primary = allowed.filter((write) => PRIMARY.includes(write.operation))
  const more = allowed.filter((write) => !PRIMARY.includes(write.operation))
  const owned = usable.filter((write) => write.refusedBy === "owned")
  if (allowed.length === 0 && owned.length === 0 && !managedInGit) return null

  return (
    <div className="flex flex-wrap items-center gap-2">
      {managedInGit ? (
        <span
          className="inline-flex items-center gap-1 rounded-sm border px-2 py-0.5 text-[12px] text-ink-muted"
          title={`Git declares its ${managedInGit.declares.join(", ")}${
            managedInGit.source ? ` — ${managedInGit.source}` : ""
          }`}
        >
          <GitBranch aria-hidden className="size-3.5" />
          managed in Git
        </span>
      ) : null}
      {primary.map((write) => {
        const Icon = ICON[write.operation]
        return (
          <Button
            key={write.operation}
            variant="outline"
            size="sm"
            onClick={() => setOpen({ kind: "write", write })}
          >
            <Icon aria-hidden />
            {LABEL[write.operation]}
          </Button>
        )
      })}
      {more.length > 0 ? (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm">
              more
              <ChevronDown aria-hidden />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            {more
              .filter((write) => write.tier !== "destroy")
              .map((write) => (
                <DropdownMenuItem
                  key={write.operation}
                  onSelect={() => setOpen({ kind: "write", write })}
                >
                  {LABEL[write.operation]}
                </DropdownMenuItem>
              ))}
            {more.some((write) => write.tier === "destroy") ? (
              <DropdownMenuSeparator />
            ) : null}
            {more
              .filter((write) => write.tier === "destroy")
              .map((write) => (
                <DropdownMenuItem
                  key={write.operation}
                  className="text-danger"
                  onSelect={() => setOpen({ kind: "write", write })}
                >
                  {LABEL[write.operation]}…
                </DropdownMenuItem>
              ))}
          </DropdownMenuContent>
        </DropdownMenu>
      ) : null}
      {owned.length > 0 ? (
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setOpen({ kind: "git" })}
        >
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
        <SheetContent className="w-full overflow-y-auto sm:max-w-xl">
          {open?.kind === "git" ? (
            <GitChanges
              connector={subject.connector}
              owned={owned}
              managedInGit={managedInGit}
            />
          ) : open?.kind === "write" ? (
            <WriteForm
              key={open.write.operation}
              subject={subject}
              write={open.write}
              task={open.task}
              onDone={() => setOpen(null)}
            />
          ) : null}
        </SheetContent>
      </Sheet>
    </div>
  )
}

/** The restart button on one task's row, where the gate allows it. */
export function TaskRestart({
  subject,
  write,
  task,
}: {
  subject: Subject
  write: WriteOption | undefined
  task: number
}) {
  const [open, setOpen] = useState(false)
  if (!write?.allowed) return null
  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        className="h-7"
        onClick={() => setOpen(true)}
      >
        <RotateCw aria-hidden />
        restart
      </Button>
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent className="w-full overflow-y-auto sm:max-w-xl">
          {open ? (
            <WriteForm
              subject={subject}
              write={write}
              task={task}
              onDone={() => setOpen(false)}
            />
          ) : null}
        </SheetContent>
      </Sheet>
    </>
  )
}

/** A config as the edit box shows it: what a read showed, `******` and all. */
function configText(config: RedactedConfig): string {
  const shown: Record<string, string> = {}
  for (const entry of config.entries) shown[entry.key] = entry.value
  return JSON.stringify(shown, null, 2)
}

/** Parse the edit box, or say what is wrong with it. */
function parseConfig(
  text: string
): { config: Record<string, string> } | { error: string } {
  try {
    const parsed: unknown = JSON.parse(text)
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return { error: "the config is a JSON object of key → value" }
    }
    const config: Record<string, string> = {}
    for (const [key, value] of Object.entries(parsed)) {
      if (typeof value !== "string") {
        return {
          error: `${key}: Connect configs are strings — write "${String(value)}"`,
        }
      }
      config[key] = value
    }
    return { config }
  } catch (error) {
    return { error: (error as Error).message }
  }
}

function parseOffsets(
  text: string
): { offsets: StoredOffset[] } | { error: string } {
  try {
    const parsed: unknown = JSON.parse(text)
    if (!Array.isArray(parsed)) {
      return { error: "offsets are a JSON array of { partition, offset }" }
    }
    return { offsets: parsed as StoredOffset[] }
  } catch (error) {
    return { error: (error as Error).message }
  }
}

function WriteForm({
  subject,
  write,
  task,
  onDone,
}: {
  subject: Subject
  write: WriteOption
  task?: number
  onDone: () => void
}) {
  const { envId, connectId, connector } = subject
  const mutation = useConnectorWrite(envId, connectId, connector)
  const navigate = useNavigate()
  const operation = write.operation
  // The safe defaults for a button: restart what has failed, tasks included.
  // On a healthy connector that restarts nothing, which is the right outcome
  // for a click nobody meant.
  const [includeTasks, setIncludeTasks] = useState(true)
  const [onlyFailed, setOnlyFailed] = useState(true)
  const [configDraft, setConfigDraft] = useState(() =>
    configText(subject.config)
  )
  const [offsetsDraft, setOffsetsDraft] = useState(() =>
    JSON.stringify(subject.offsets ?? [], null, 2)
  )
  const [confirm, setConfirm] = useState("")
  const [verdict, setVerdict] = useState<WriteResult | null>(null)
  const destroy = write.tier === "destroy"
  const failure = mutation.error as ApiError | null
  const needsStop =
    (operation === "connector.reset_offsets" ||
      operation === "connector.alter_offsets") &&
    subject.status !== "stopped"

  const parsedConfig =
    operation === "connector.edit" ? parseConfig(configDraft) : null
  const parsedOffsets =
    operation === "connector.alter_offsets" ? parseOffsets(offsetsDraft) : null
  const inputError =
    (parsedConfig && "error" in parsedConfig && parsedConfig.error) ||
    (parsedOffsets && "error" in parsedOffsets && parsedOffsets.error) ||
    null

  const send = (dryRun: boolean) => {
    mutation.mutate(
      {
        operation,
        task,
        request: {
          ...(operation === "connector.restart"
            ? { includeTasks, onlyFailed }
            : {}),
          ...(parsedConfig && "config" in parsedConfig
            ? { config: parsedConfig.config }
            : {}),
          ...(parsedOffsets && "offsets" in parsedOffsets
            ? { offsets: parsedOffsets.offsets }
            : {}),
          ...(destroy ? { confirm } : {}),
          ...(dryRun ? { dryRun: true } : {}),
        },
      },
      {
        onSuccess: (result) => {
          if (!result.applied) setVerdict(result)
          if (result.applied && operation === "connector.delete") {
            void navigate({
              to: "/environments/$envId/connect-clusters/$connectId",
              params: { envId, connectId },
              search: { listing: "connectors" },
            })
          }
        },
      }
    )
  }

  const applied = mutation.isSuccess && mutation.data.applied
  const equivalent =
    (mutation.isSuccess && mutation.data.applied
      ? mutation.data.equivalent
      : null) ??
    verdict?.equivalent ??
    write.equivalent

  return (
    <>
      <SheetHeader>
        <SheetTitle>
          {LABEL[operation]}
          {task !== undefined ? ` ${task}` : ""} of{" "}
          <span className="font-mono">{connector}</span>
        </SheetTitle>
        <SheetDescription>{WHAT[operation]}</SheetDescription>
      </SheetHeader>

      <div className="space-y-4 px-4">
        {operation === "connector.restart" ? (
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

        {operation === "connector.edit" ? (
          <ConfigBox
            value={configDraft}
            onChange={(next) => {
              setConfigDraft(next)
              setVerdict(null)
            }}
            hint="Secrets show as ****** and are kept as they are unless you replace them. A key you remove is removed."
          />
        ) : null}

        {operation === "connector.alter_offsets" ? (
          <ConfigBox
            value={offsetsDraft}
            onChange={setOffsetsDraft}
            hint="The stored offsets as the worker reports them. Change an offset, or set it to null to clear that partition's."
          />
        ) : null}

        {needsStop ? (
          <p className="text-[13px] text-warn-ink">
            This connector is {subject.status}; the worker refuses offset
            changes until it is stopped. Stop it first.
          </p>
        ) : null}

        {inputError ? (
          <p className="text-[13px] text-danger">{inputError}</p>
        ) : null}

        {verdict?.validation ? (
          <ValidationList validation={verdict.validation} />
        ) : null}

        <EquivalentBlock equivalent={equivalent} />

        {destroy ? (
          <ConfirmName name={connector} value={confirm} onChange={setConfirm} />
        ) : null}

        <p className="text-[12px] text-ink-faint">
          Recorded in the audit before it is sent, with your name and roles; if
          it cannot be recorded, it is not sent.
        </p>

        {applied ? (
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
        {applied ? (
          <Button onClick={onDone}>close</Button>
        ) : (
          <div className="flex flex-wrap justify-end gap-2">
            {operation === "connector.edit" ? (
              <Button
                variant="outline"
                disabled={mutation.isPending || !!inputError}
                onClick={() => send(true)}
              >
                validate
              </Button>
            ) : null}
            <Button
              variant={
                destroy || operation === "connector.stop"
                  ? "destructive"
                  : "default"
              }
              disabled={
                mutation.isPending ||
                !!inputError ||
                (destroy && confirm !== connector)
              }
              onClick={() => send(false)}
            >
              {mutation.isPending ? "sending…" : LABEL[operation]}
            </Button>
          </div>
        )}
      </SheetFooter>
    </>
  )
}

/** "New connector" on a Connect cluster page, where the gate allows it. */
export function CreateConnector({
  envId,
  connectId,
  write,
}: {
  envId: string
  connectId: string
  write: WriteOption | null
}) {
  const [open, setOpen] = useState(false)
  if (!write?.allowed) return null
  return (
    <>
      <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
        <Plus aria-hidden />
        new connector
      </Button>
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent className="w-full overflow-y-auto sm:max-w-xl">
          {open ? (
            <CreateForm
              envId={envId}
              connectId={connectId}
              write={write}
              onDone={() => setOpen(false)}
            />
          ) : null}
        </SheetContent>
      </Sheet>
    </>
  )
}

function CreateForm({
  envId,
  connectId,
  write,
  onDone,
}: {
  envId: string
  connectId: string
  write: WriteOption
  onDone: () => void
}) {
  const plugins = useConnectPlugins(envId, connectId)
  const mutation = useConnectorCreate(envId, connectId)
  const navigate = useNavigate()
  const [name, setName] = useState("")
  const [draft, setDraft] = useState(
    JSON.stringify({ "connector.class": "", "tasks.max": "1" }, null, 2)
  )
  const [verdict, setVerdict] = useState<WriteResult | null>(null)
  const parsed = parseConfig(draft)
  const failure = mutation.error as ApiError | null
  const inputError = "error" in parsed ? parsed.error : null

  const choosePlugin = (plugin: string) => {
    const current = "config" in parsed ? parsed.config : {}
    setDraft(JSON.stringify({ ...current, "connector.class": plugin }, null, 2))
    setVerdict(null)
  }

  const send = (dryRun: boolean) => {
    if (!("config" in parsed)) return
    mutation.mutate(
      { name: name.trim(), config: parsed.config, dryRun },
      {
        onSuccess: (result) => {
          if (!result.applied) {
            setVerdict(result)
            return
          }
          onDone()
          void navigate({
            to: "/environments/$envId/connect-clusters/$connectId/connectors/$connector",
            params: { envId, connectId, connector: result.connector },
            search: { pane: "tasks" },
          })
        },
      }
    )
  }

  return (
    <>
      <SheetHeader>
        <SheetTitle>new connector</SheetTitle>
        <SheetDescription>{WHAT["connector.create"]}</SheetDescription>
      </SheetHeader>
      <div className="space-y-4 px-4">
        <label className="block space-y-1 text-[13px]">
          <span className="text-ink-muted">name</span>
          <Input
            value={name}
            onChange={(event) => {
              setName(event.target.value)
              setVerdict(null)
            }}
            className="h-8 font-mono"
          />
        </label>
        <div className="space-y-1 text-[13px]">
          <span className="text-ink-muted">plugin</span>
          <Select onValueChange={choosePlugin}>
            <SelectTrigger className="w-full" aria-label="connector plugin">
              <SelectValue placeholder="choose an installed plugin" />
            </SelectTrigger>
            <SelectContent>
              {(plugins.data?.plugins ?? []).map((plugin) => (
                <SelectItem key={plugin.class} value={plugin.class}>
                  <span className="font-mono text-[12px]">{plugin.class}</span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <ConfigBox
          value={draft}
          onChange={(next) => {
            setDraft(next)
            setVerdict(null)
          }}
          hint="Prefer a config-provider reference — ${file:…} or ${secrets:…} — to a secret typed here."
        />
        {inputError ? (
          <p className="text-[13px] text-danger">{inputError}</p>
        ) : null}
        {verdict?.validation ? (
          <ValidationList validation={verdict.validation} />
        ) : null}
        <EquivalentBlock equivalent={verdict?.equivalent ?? write.equivalent} />
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
        <div className="flex flex-wrap justify-end gap-2">
          <Button
            variant="outline"
            disabled={mutation.isPending || !!inputError || !name.trim()}
            onClick={() => send(true)}
          >
            validate
          </Button>
          <Button
            disabled={mutation.isPending || !!inputError || !name.trim()}
            onClick={() => send(false)}
          >
            {mutation.isPending ? "sending…" : "create"}
          </Button>
        </div>
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
          Git declares this connector&rsquo;s{" "}
          {managedInGit?.declares.join(", ") ?? "state"}, so those are changed
          there — whatever applies Git would put back a change made here.
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

function ConfigBox({
  value,
  onChange,
  hint,
}: {
  value: string
  onChange: (next: string) => void
  hint: string
}) {
  return (
    <div className="space-y-1">
      <textarea
        value={value}
        onChange={(event) => onChange(event.target.value)}
        spellCheck={false}
        rows={14}
        className="w-full rounded-sm border bg-transparent p-3 font-mono text-[12px] leading-relaxed"
      />
      <p className="text-[12px] text-ink-faint">{hint}</p>
    </div>
  )
}

function ConfirmName({
  name,
  value,
  onChange,
}: {
  name: string
  value: string
  onChange: (next: string) => void
}) {
  return (
    <label className="block space-y-1 text-[13px]">
      <span className="text-danger">
        This cannot be undone. Type <Mono>{name}</Mono> to confirm.
      </span>
      <Input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="h-8 font-mono"
        aria-label="connector name, to confirm"
      />
    </label>
  )
}

function ValidationList({ validation }: { validation: Validation }) {
  if (validation.errorCount === 0) {
    return (
      <p className="text-[13px] text-ok">
        The plugin accepts this config. Nothing was applied.
      </p>
    )
  }
  return (
    <div className="space-y-1 text-[13px]">
      <p className="text-danger">
        The plugin found {validation.errorCount} problem
        {validation.errorCount === 1 ? "" : "s"}. Nothing was applied.
      </p>
      <ul className="space-y-1">
        {validation.errors.map((error) => (
          <li key={error.key}>
            <Mono>{error.key}</Mono>{" "}
            <span className="text-ink-muted">{error.messages.join("; ")}</span>
          </li>
        ))}
      </ul>
    </div>
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
