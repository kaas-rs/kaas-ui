/**
 * A number in one of its three states, as text.
 *
 * The same rule the topic table's `Metric` cell draws, for the same reason:
 * blank means the fan-out is still out, and an em dash means it came back
 * without a number. A dash that quietly means "still loading" is how a cluster
 * looks broken for as long as it is slow.
 */
export function pending(
  value: number | null,
  render: (value: number) => string,
  fetching: boolean
): string {
  if (value !== null) return render(value)
  return fetching ? "\u00b7" : "\u2014"
}
