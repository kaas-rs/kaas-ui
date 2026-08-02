// The one place a row's identity is computed.
//
// `{partition}-{offset}` is the key in `getItemKey`, in React, in the
// selection state and in the detail query key. Computing it in four places is
// how those four end up disagreeing, so it is computed once, on arrival, and
// carried on the row.
//
// It is not sent on the wire: partition and offset are already there, and at
// ten thousand rows a second a redundant string per row is bytes the browser
// parses to learn nothing.

import type { MalformedRow, StreamRecord, StreamRow, StreamRowData } from "@/api/types";

export function rowId(row: StreamRowData): string {
  return `${row.partition}-${row.offset}`;
}

export function withId(row: StreamRowData): StreamRow {
  return { ...row, id: rowId(row) };
}

export function withIds(rows: StreamRowData[]): StreamRow[] {
  return rows.map(withId);
}

export function isRecord(row: StreamRow): row is StreamRecord & { id: string } {
  return row.kind === "record";
}

export function isMalformed(row: StreamRow): row is MalformedRow & { id: string } {
  return row.kind === "malformed";
}

/** Split an id back into the pair the single-message route takes. */
export function parseRowId(id: string): { partition: number; offset: number } | null {
  const separator = id.indexOf("-");
  if (separator < 1) return null;
  const partition = Number(id.slice(0, separator));
  const offset = Number(id.slice(separator + 1));
  if (!Number.isInteger(partition) || !Number.isInteger(offset)) return null;
  return { partition, offset };
}
