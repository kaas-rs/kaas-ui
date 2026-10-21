// The one dialog every write goes through.
//
// Phase 10's `WriteDialog`: a form's own fields, framed the same way for
// every write kaas-ui can make — title and what it does, the fields, the
// audit note, a typed name for anything that cannot be undone, then the
// outcome in the same place: the audit id, or the server's refusal in its
// own words. A form supplies its fields and what submitting sends; the
// look, the buttons and the order they come in are this file's.
//
// An alert dialog, not a sheet: a write is a decision, and it does not close
// on a click outside it. Submit and validate are plain buttons rather than
// `AlertDialogAction`, which would close the dialog before the result could
// be shown in it.

import { useState, type ReactNode } from "react"

import { Mono } from "@/components/domain"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"

export interface WriteDialogProps {
  /** Dismissed: cancel, close, or Escape. */
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
  /** Set once the write was applied: the dialog says so and offers close. */
  auditId?: string | null
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
  auditId,
}: WriteDialogProps) {
  const [typed, setTyped] = useState("")
  const confirmed = confirmName === undefined || typed === confirmName
  const done = !!auditId

  return (
    <AlertDialog
      open
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <AlertDialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-xl">
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>

        <div className="min-w-0 space-y-4">
          {children}

          {confirmName !== undefined && !done ? (
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

          {done ? (
            <p className="text-[13px] text-ok">
              Done. Audit <Mono>{auditId}</Mono>.
            </p>
          ) : null}
          {error ? <p className="text-[13px] text-danger">{error}</p> : null}
        </div>

        <AlertDialogFooter>
          {done ? (
            <AlertDialogAction>close</AlertDialogAction>
          ) : (
            <>
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
            </>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
