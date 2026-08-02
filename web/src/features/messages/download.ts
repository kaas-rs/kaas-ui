// The buffer, as a file.
//
// A read-only tool that can show you ten thousand records and give you none of
// them is halfway to useless: the next thing anyone wants is to diff two of
// them, or grep, or hand the lot to a colleague. This is the smallest honest
// version of that — what the list is holding, exactly as it is holding it.
//
// **These are previews, not whole records.** The list carries a 256-character
// payload preview; the rest is fetched for the one record someone selects.
// Every `Payload` says so itself with `truncated`, and the document repeats it
// in a field of its own, because a file outlives the window it was taken from
// and nobody reads the UI copy that was on screen at the time.

import type { StreamRow } from "@/api/types";
import type { SeekMode } from "./seek-modes";

export interface BufferExport {
  cluster: string;
  topic: string;
  /** The seek mode the window was read with. */
  mode: SeekMode;
  exportedAt: string;
  count: number;
  /** Said out loud, so the file cannot be mistaken for the whole record. */
  payloads: string;
  messages: StreamRow[];
}

export function bufferExport(
  clusterId: string,
  topic: string,
  mode: SeekMode,
  rows: StreamRow[],
): BufferExport {
  return {
    cluster: clusterId,
    topic,
    mode,
    exportedAt: new Date().toISOString(),
    count: rows.length,
    payloads:
      "previews — where a payload has truncated: true, the record holds more than its text shows",
    messages: rows,
  };
}

/** `kaas-canary-newest-2026-08-02T14-32-11-004Z.json`. */
export function exportFilename(topic: string, mode: SeekMode, at: Date): string {
  // Kafka allows only word characters, dot and dash in a topic name, so this
  // is a belt rather than a fix — but a filename is not the place to find out
  // that an assumption about someone else's namespace was wrong.
  const safe = topic.replace(/[^\w.-]/g, "_");
  return `${safe}-${mode}-${at.toISOString().replace(/[:.]/g, "-")}.json`;
}

/**
 * Hand the document to the browser as a download.
 *
 * The object URL is revoked on a timer rather than immediately after the
 * click: the click starts the download asynchronously, and revoking in the
 * same tick cancels it in some browsers — a bug that reproduces on someone
 * else's machine and never on the one it was written on.
 */
export function downloadBuffer(
  clusterId: string,
  topic: string,
  mode: SeekMode,
  rows: StreamRow[],
): void {
  const document_ = bufferExport(clusterId, topic, mode, rows);
  const blob = new Blob([JSON.stringify(document_, null, 2)], {
    type: "application/json",
  });
  const url = URL.createObjectURL(blob);

  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = exportFilename(topic, mode, new Date());
  anchor.style.display = "none";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();

  window.setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
