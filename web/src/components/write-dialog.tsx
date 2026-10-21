// The one dialog every write goes through.
//
// Phase 10's `WriteDialog`: a form's own fields, framed the same way for
// every write kaas-ui can make — title and what it does, the fields, the
// audit note, a typed name for anything that cannot be undone. A form
// supplies its fields and what submitting sends; the look, the buttons and
// the order they come in are this file's.
//
// What happens next is this file's too. Applied, the dialog closes and
// `toastApplied` says so — one toast for every write. Refused, the dialog
// stays open with the server's words in it, beside the fields that may need
// changing for a second try.
//
// An alert dialog, not a sheet: a write is a decision, and it does not close
// on a click outside it. Submit and validate are plain buttons rather than
// `AlertDialogAction`, which would close the dialog before a refusal could
// be shown in it.

import { useState, type ReactNode } from "react"

import { Mono } from "@/components/domain"
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { toast } from "@/components/ui/toast"

export interface WriteDialogProps {
  /** Dismissed — cancel or Escape — or applied. */
  onClose: () => void
  title: ReactNode
  /** What the write does, in a sentence. */
  description: ReactNode
  /** The form's own fields, and anything only this form has to say. */
  children?: ReactNode
  /** The submit button's word: `restart`, `delete`, `create`. */
  action: string
  /** Red, for a write that stops or loses something. */
  destructive?: boolean
  /** For a write that cannot be undone: the name to type before submitting. */
  confirmName?: string
  /** Offer a dry run beside the submit. */
  onValidate?: () => void
  /** Whether the fields are complete enough to validate. Default true. */
  canValidate?: boolean
  /** Send it. `confirm` is the typed name, where `confirmName` asked for one. */
  onSubmit: (confirm?: string) => void
  /** Whether the fields are complete enough to send. Default true. */
  canSubmit?: boolean
  /** A request is in flight. */
  pending: boolean
  /** What the last attempt ended in, verbatim. */
  error?: string | null
}

/**
 * A write the server applied, after its dialog has closed.
 *
 * "Accepted", not "done": a Connect worker answers most writes before it has
 * acted on them, and the page's own refresh shows the effect. The audit id
 * joins this toast to the intent and outcome lines in the audit log.
 */
export function toastApplied(what: string, auditId: string | null) {
  toast.add({
    type: "success",
    title: what,
    description: auditId ? `Accepted. Audit ${auditId}.` : "Accepted.",
  })
}

/** Always open: mount it to open the dialog and unmount it to close, so the
    form around it starts from fresh drafts every time. */
export function WriteDialog({
  onClose,
  title,
  description,
  children,
  action,
  destructive = false,
  confirmName,
  onValidate,
  canValidate = true,
  onSubmit,
  canSubmit = true,
  pending,
  error,
}: WriteDialogProps) {
  const [typed, setTyped] = useState("")
  const confirmed = confirmName === undefined || typed === confirmName

  return (
    <AlertDialog
      open
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <AlertDialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto data-[size=default]:max-w-[calc(100%-2rem)] data-[size=default]:sm:max-w-xl">
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>

        <div className="min-w-0 space-y-4">
          {children}

          {confirmName !== undefined ? (
            <label className="block space-y-1 text-[13px]">
              <span className="text-danger">
                This cannot be undone. Type <Mono>{confirmName}</Mono> to
                confirm.
              </span>
              <Input
                value={typed}
                onChange={(event) => setTyped(event.target.value)}
                className="h-8 font-mono"
                aria-label={`type ${confirmName} to confirm`}
              />
            </label>
          ) : null}

          <p className="text-[12px] text-ink-faint">
            Recorded in the audit before it is sent, with your name and roles;
            if it cannot be recorded, it is not sent.
          </p>

          {error ? <p className="text-[13px] text-danger">{error}</p> : null}
        </div>

        <AlertDialogFooter>
          <AlertDialogCancel disabled={pending}>cancel</AlertDialogCancel>
          {onValidate ? (
            <Button
              variant="outline"
              disabled={pending || !canValidate}
              onClick={onValidate}
            >
              validate
            </Button>
          ) : null}
          <Button
            variant={destructive ? "destructive" : "default"}
            disabled={pending || !canSubmit || !confirmed}
            onClick={() =>
              onSubmit(confirmName === undefined ? undefined : typed)
            }
          >
            {pending ? "sending…" : action}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
