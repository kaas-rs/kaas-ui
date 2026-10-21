// The writes a connector can take, each in `WriteDialog`.
//
// Phase 10. Two things decide what appears here, both on the server: the
// Connect cluster's `writes.allow` ceiling and the caller's roles. A button
// is drawn only for a write both agree to — a button that 403s is worse than
// none.
//
// The tiers decide the friction, never the permission: operate is one click
// and a confirm, edit shows what will change, destroy wants the connector's
// name typed.

import { useState } from "react"
import { useNavigate } from "@tanstack/react-router"
import {
  ChevronDown,
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
  Operation,
  RedactedConfig,
  StoredOffset,
  Validation,
  WriteOption,
  WriteResult,
} from "@/api/types"
import { Mono } from "@/components/domain"
import { toastApplied, WriteDialog } from "@/components/write-dialog"
import { Button } from "@/components/ui/button"
import { ButtonGroup } from "@/components/ui/button-group"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
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

/** What each operation does, in one sentence, for the dialog's description. */
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

/** What the dialog needs to know about the connector it is changing. */
interface Subject {
  envId: string
  connectId: string
  connector: string
  status: ConnectorStatus
  config: RedactedConfig
  offsets: StoredOffset[] | null
}

type Open = { write: WriteOption; task?: number } | null

export function ConnectorWrites({
  writes,
  ...subject
}: Subject & {
  writes: WriteOption[]
}) {
  const [open, setOpen] = useState<Open>(null)
  const allowed = writes.filter(
    (write) => write.allowed && fits(write.operation, subject.status)
  )
  // Everything that can be undone is a segment of the group, in catalogue
  // order; what cannot — delete, the offsets — is one step further away, in
  // the group's last segment.
  const inline = allowed.filter((write) => write.tier !== "destroy")
  const destroy = allowed.filter((write) => write.tier === "destroy")
  if (allowed.length === 0) return null

  return (
    <>
      <ButtonGroup aria-label="connector writes">
        {inline.map((write) => {
          const Icon = ICON[write.operation]
          return (
            <Button
              key={write.operation}
              variant="outline"
              size="sm"
              onClick={() => setOpen({ write })}
            >
              <Icon aria-hidden />
              {/* Icons alone on a phone: five labelled segments do not fit. */}
              <span className="max-sm:sr-only">{LABEL[write.operation]}</span>
            </Button>
          )
        })}
        {destroy.length > 0 ? (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm" aria-label="more writes">
                more
                <ChevronDown aria-hidden />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {destroy.map((write) => {
                const Icon = ICON[write.operation]
                return (
                  <DropdownMenuItem
                    key={write.operation}
                    className="text-danger"
                    onSelect={() => setOpen({ write })}
                  >
                    <Icon aria-hidden />
                    {LABEL[write.operation]}…
                  </DropdownMenuItem>
                )
              })}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
      </ButtonGroup>

      {open ? (
        <WriteForm
          key={open.write.operation}
          subject={subject}
          write={open.write}
          task={open.task}
          onClose={() => setOpen(null)}
        />
      ) : null}
    </>
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
      {open ? (
        <WriteForm
          subject={subject}
          write={write}
          task={task}
          onClose={() => setOpen(false)}
        />
      ) : null}
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
  onClose,
}: {
  subject: Subject
  write: WriteOption
  task?: number
  onClose: () => void
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

  const send = (dryRun: boolean, confirm?: string) => {
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
          ...(confirm !== undefined ? { confirm } : {}),
          ...(dryRun ? { dryRun: true } : {}),
        },
      },
      {
        onSuccess: (result) => {
          if (!result.applied) {
            setVerdict(result)
            return
          }
          toastApplied(
            `${LABEL[operation]}${task !== undefined ? ` ${task}` : ""} · ${connector}`,
            result.auditId
          )
          onClose()
          if (operation === "connector.delete") {
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

  return (
    <WriteDialog
      onClose={onClose}
      title={
        <>
          {LABEL[operation]}
          {task !== undefined ? ` ${task}` : ""} of{" "}
          <span className="font-mono">{connector}</span>
        </>
      }
      description={WHAT[operation]}
      action={LABEL[operation]}
      destructive={destroy || operation === "connector.stop"}
      confirmName={destroy ? connector : undefined}
      onValidate={operation === "connector.edit" ? () => send(true) : undefined}
      canValidate={!inputError}
      onSubmit={(confirm) => send(false, confirm)}
      canSubmit={!inputError}
      pending={mutation.isPending}
      error={failure?.message}
    >
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
          This connector is {subject.status}; the worker refuses offset changes
          until it is stopped. Stop it first.
        </p>
      ) : null}

      {inputError ? (
        <p className="text-[13px] text-danger">{inputError}</p>
      ) : null}

      {verdict?.validation ? (
        <ValidationList validation={verdict.validation} />
      ) : null}
    </WriteDialog>
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
      {open ? (
        <CreateForm
          envId={envId}
          connectId={connectId}
          onClose={() => setOpen(false)}
        />
      ) : null}
    </>
  )
}

function CreateForm({
  envId,
  connectId,
  onClose,
}: {
  envId: string
  connectId: string
  onClose: () => void
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
  const ready = !inputError && !!name.trim()

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
          toastApplied(`create · ${result.connector}`, result.auditId)
          onClose()
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
    <WriteDialog
      onClose={onClose}
      title="new connector"
      description={WHAT["connector.create"]}
      action="create"
      onValidate={() => send(true)}
      canValidate={ready}
      onSubmit={() => send(false)}
      canSubmit={ready}
      pending={mutation.isPending}
      error={failure?.message}
    >
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
    </WriteDialog>
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
