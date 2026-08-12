/**
 * A compatibility mode, and where it came from.
 *
 * `BACKWARD` set on this subject and `BACKWARD` inherited from the registry
 * are not the same fact — the second changes when somebody edits the registry
 * default, and the first does not.
 */
export function Compatibility({
  mode,
  inherited,
}: {
  mode: string
  inherited: boolean
}) {
  return (
    <span className="flex items-center gap-1.5">
      <span className="font-mono text-[12px]">{mode}</span>
      {inherited ? (
        <span
          className="text-[11px] text-ink-faint"
          title="Inherited from the registry default — this subject sets no rule of its own"
        >
          inherited
        </span>
      ) : null}
    </span>
  )
}
