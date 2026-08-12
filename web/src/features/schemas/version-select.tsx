import type { SubjectSchema } from "@/api/types"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

export function VersionSelect({
  label,
  versions,
  value,
  onChange,
}: {
  label: string
  versions: SubjectSchema[]
  value?: number
  onChange(version: number): void
}) {
  // Oldest first, so the last one is the version in force. Derived rather than
  // passed: one list, one notion of which end of it is current.
  const current = versions[versions.length - 1]?.version

  return (
    <Label className="gap-1 text-xs font-normal text-ink-faint">
      {label}
      <Select
        value={value !== undefined ? String(value) : undefined}
        onValueChange={(next) => onChange(Number(next))}
      >
        <SelectTrigger className="w-[130px]" aria-label={`${label} version`}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {versions.map((version) => (
            <SelectItem key={version.version} value={String(version.version)}>
              {/* The version, and whether it is the one in force. The schema
                  id used to ride along here and does not belong: it is a
                  registry-wide counter, so `v1 (#2)` invites reading 2 as
                  something about this subject when it is a position in a
                  sequence shared with every other subject. It is on the
                  overview, once, where it is the only number of its kind. */}
              v{version.version}
              {version.version === current ? (
                <span className="text-ink-faint">(current)</span>
              ) : null}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Label>
  )
}
