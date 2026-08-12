// Number formatting shared by every page that renders a broker's answer.

export function bytes(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—"
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"]
  let size = value
  let unit = 0
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024
    unit += 1
  }
  return `${size < 10 && unit > 0 ? size.toFixed(1) : Math.round(size)} ${units[unit]}`
}

export function count(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—"
  return value.toLocaleString()
}

export function duration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`
  const seconds = ms / 1000
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`
  const minutes = seconds / 60
  if (minutes < 60) return `${Math.round(minutes)}m`
  return `${Math.round(minutes / 60)}h`
}
