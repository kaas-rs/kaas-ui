import { highlight } from "@/lib/highlight"

import { prettyJson } from "./pretty-json"

/**
 * The schema text.
 *
 * JSON — which Avro and JSON Schema both are — is pretty-printed so the
 * registry's own whitespace does not decide readability. Protobuf is `.proto`
 * source and is shown as it was registered.
 */
export function SchemaText({ text, format }: { text: string; format: string }) {
  const proto = format === "protobuf"
  const shown = proto ? text : prettyJson(text)
  return (
    <pre className="max-h-[45vh] overflow-auto rounded-md border border-line bg-surface-sunken p-3 font-mono text-[11px] leading-relaxed whitespace-pre">
      {/* Coloured by the declared format rather than by sniffing the text: the
          registry says which of the three this is, and a JSON schema that
          happens to start with a brace is not a reason to guess. */}
      {highlight(shown, proto ? "proto" : "json")}
    </pre>
  )
}
