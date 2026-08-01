import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Card, Empty, Mono, Spinner } from "../components";
import { PageTitle } from "../shell";

/* The subset of OpenAPI 3.1 this document actually uses. Typing it honestly
 * rather than as `any` is what lets the renderer below say "this field is
 * nullable" instead of guessing. */

interface Schema {
  type?: string | string[];
  format?: string;
  description?: string;
  properties?: Record<string, Schema>;
  required?: string[];
  items?: Schema;
  $ref?: string;
  enum?: string[];
  oneOf?: Schema[];
  allOf?: Schema[];
  minimum?: number;
}

interface Parameter {
  name: string;
  in: "path" | "query" | "header";
  description?: string;
  required?: boolean;
  schema?: Schema;
}

interface Operation {
  tags?: string[];
  summary?: string;
  description?: string;
  operationId?: string;
  parameters?: Parameter[];
  responses?: Record<
    string,
    { description?: string; content?: Record<string, { schema?: Schema }> }
  >;
}

interface Spec {
  info: { title: string; version: string; description?: string };
  paths: Record<string, Record<string, Operation>>;
  components?: { schemas?: Record<string, Schema> };
  tags?: { name: string; description?: string }[];
}

function refName(ref: string | undefined): string | null {
  if (!ref) return null;
  const name = ref.split("/").pop();
  return name ?? null;
}

/** A one-line rendering of a schema, for a parameter or a property. */
function typeOf(schema: Schema | undefined): string {
  if (!schema) return "—";
  if (schema.$ref) return refName(schema.$ref) ?? "object";
  if (schema.oneOf) {
    return schema.oneOf.map(typeOf).join(" | ");
  }
  if (Array.isArray(schema.type)) {
    // OpenAPI 3.1 spells nullable as a type union, which is worth showing
    // as-is: `string | null` is the difference between "absent" and "empty".
    return schema.type.join(" | ");
  }
  if (schema.type === "array") {
    return `${typeOf(schema.items)}[]`;
  }
  if (schema.enum) {
    return schema.enum.map((value) => `"${value}"`).join(" | ");
  }
  return schema.type ?? "object";
}

export function ApiDocs() {
  const spec = useQuery({
    queryKey: ["openapi"],
    queryFn: async (): Promise<Spec> => {
      const response = await fetch("/api/openapi.json");
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
      return (await response.json()) as Spec;
    },
    staleTime: Infinity,
  });

  const [open, setOpen] = useState<string | null>(null);

  const byTag = useMemo(() => {
    const grouped = new Map<string, { path: string; method: string; op: Operation }[]>();
    for (const [path, methods] of Object.entries(spec.data?.paths ?? {})) {
      for (const [method, op] of Object.entries(methods)) {
        const tag = op.tags?.[0] ?? "other";
        grouped.set(tag, [...(grouped.get(tag) ?? []), { path, method, op }]);
      }
    }
    return grouped;
  }, [spec.data]);

  if (spec.isLoading) return <Spinner label="reading the document" />;
  if (spec.error || !spec.data) {
    return (
      <Card className="p-5 text-[13px] text-danger">
        the OpenAPI document could not be read: {String(spec.error)}
      </Card>
    );
  }

  const schemas = spec.data.components?.schemas ?? {};
  const tagOrder = spec.data.tags?.map((tag) => tag.name) ?? [...byTag.keys()];
  const description = new Map(
    (spec.data.tags ?? []).map((tag) => [tag.name, tag.description]),
  );

  const total = [...byTag.values()].reduce((sum, ops) => sum + ops.length, 0);

  return (
    <>
      <PageTitle
        title="API"
        subtitle={`${total} endpoints, every one a GET`}
        actions={
          <a
            href="/api/openapi.json"
            className="text-[13px] hover:underline"
            style={{ color: "var(--color-link)" }}
          >
            openapi.json →
          </a>
        }
      />

      <Card className="p-4 mb-6 text-[13px] text-ink-muted max-w-3xl">
        {spec.data.info.description}
        <p className="mt-2">
          There is no "try it" console because there is nothing to configure: a
          request is a URL, and every path below is a link you can click.
        </p>
      </Card>

      {tagOrder
        .filter((tag) => byTag.has(tag))
        .map((tag) => (
          <section key={tag} className="mb-8">
            <div className="flex items-baseline gap-3 mb-3">
              <h2 className="text-[15px] font-semibold tracking-[-0.01em]">{tag}</h2>
              <span className="text-[12px] text-ink-muted">{description.get(tag)}</span>
            </div>

            <div className="flex flex-col gap-2">
              {(byTag.get(tag) ?? []).map(({ path, method, op }) => {
                const key = `${method} ${path}`;
                const expanded = open === key;
                return (
                  <Card key={key} className="overflow-hidden">
                    <button
                      type="button"
                      onClick={() => setOpen(expanded ? null : key)}
                      aria-expanded={expanded}
                      className="w-full flex items-center gap-3 px-3 py-2 text-left hover:bg-surface-sunken"
                    >
                      <span
                        className="text-[11px] font-semibold px-1.5 py-0.5 rounded-sm uppercase"
                        style={{ background: "var(--color-ok-soft)", color: "var(--color-ok)" }}
                      >
                        {method}
                      </span>
                      <span className="font-mono text-[13px]">{path}</span>
                      <span className="text-[12px] text-ink-faint ml-auto">
                        {expanded ? "−" : "+"}
                      </span>
                    </button>

                    {expanded ? (
                      <div className="px-3 pb-3 pt-1 border-t border-line">
                        <Detail path={path} op={op} schemas={schemas} />
                      </div>
                    ) : null}
                  </Card>
                );
              })}
            </div>
          </section>
        ))}
    </>
  );
}

function Detail({
  path,
  op,
  schemas,
}: {
  path: string;
  op: Operation;
  schemas: Record<string, Schema>;
}) {
  const parameters = op.parameters ?? [];
  const responses = Object.entries(op.responses ?? {});

  // A path with no path parameters is directly openable; one with `{id}` in it
  // is not, and offering a dead link would be worse than offering none.
  const openable = !path.includes("{");

  return (
    <div className="text-[13px]">
      {op.description ? (
        <p className="text-ink-muted mb-3 whitespace-pre-line">{op.description}</p>
      ) : null}

      {openable ? (
        <p className="mb-3">
          <a
            href={path}
            className="font-mono hover:underline"
            style={{ color: "var(--color-link)" }}
          >
            {path} ↗
          </a>
        </p>
      ) : null}

      {parameters.length > 0 ? (
        <>
          <h4 className="text-[12px] uppercase tracking-wide text-ink-faint mb-1.5">
            parameters
          </h4>
          <table className="w-full mb-4 border-collapse">
            <tbody>
              {parameters.map((parameter) => (
                <tr key={`${parameter.in}-${parameter.name}`} className="align-top">
                  <td className="py-1 pr-3 whitespace-nowrap">
                    <span className="font-mono">{parameter.name}</span>
                    {parameter.required ? (
                      <span className="text-danger ml-1" title="required">
                        *
                      </span>
                    ) : null}
                  </td>
                  <td className="py-1 pr-3 text-ink-faint">{parameter.in}</td>
                  <td className="py-1 pr-3">
                    <Mono>{typeOf(parameter.schema)}</Mono>
                  </td>
                  <td className="py-1 text-ink-muted">{parameter.description}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      ) : null}

      <h4 className="text-[12px] uppercase tracking-wide text-ink-faint mb-1.5">
        responses
      </h4>
      <div className="flex flex-col gap-2">
        {responses.map(([status, response]) => {
          const schema = response.content?.["application/json"]?.schema;
          const name = refName(schema?.$ref);
          return (
            <div key={status}>
              <span
                className="font-mono text-[12px] px-1.5 py-0.5 rounded-sm mr-2"
                style={
                  status.startsWith("2")
                    ? { background: "var(--color-ok-soft)", color: "var(--color-ok)" }
                    : { background: "var(--color-warn-soft)", color: "var(--color-warn-ink)" }
                }
              >
                {status}
              </span>
              <span className="text-ink-muted">{response.description}</span>
              {name && schemas[name] ? (
                <div className="mt-1.5 ml-1 border-l-2 border-line pl-3">
                  <SchemaTree schema={schemas[name]} schemas={schemas} name={name} />
                </div>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** One level of a schema, with `$ref`s resolved on demand. */
function SchemaTree({
  schema,
  schemas,
  name,
  depth = 0,
}: {
  schema: Schema;
  schemas: Record<string, Schema>;
  name?: string;
  depth?: number;
}) {
  const [open, setOpen] = useState(depth === 0);

  const resolved = schema.$ref
    ? (schemas[refName(schema.$ref) ?? ""] ?? schema)
    : schema;
  const properties = resolved.properties;

  if (!properties) {
    return <Mono>{typeOf(schema)}</Mono>;
  }

  // Nesting is bounded: past three levels the tree is longer than the JSON it
  // describes, and openapi.json is one click away.
  if (depth > 2) {
    return <Mono>{name ?? typeOf(schema)}</Mono>;
  }

  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="text-[12px] font-mono text-ink-muted hover:text-ink"
      >
        {open ? "▾" : "▸"} {name ?? "object"}
      </button>
      {open ? (
        <table className="w-full mt-1 border-collapse">
          <tbody>
            {Object.entries(properties).map(([property, value]) => {
              const nested =
                value.$ref ??
                (value.type === "array" ? value.items?.$ref : undefined);
              const nestedName = refName(nested);
              const nestedSchema = nestedName ? schemas[nestedName] : undefined;

              return (
                <tr key={property} className="align-top">
                  <td className="py-0.5 pr-3 whitespace-nowrap">
                    <span className="font-mono text-[12px]">{property}</span>
                    {resolved.required?.includes(property) ? (
                      <span className="text-danger ml-1" title="always present">
                        *
                      </span>
                    ) : null}
                  </td>
                  <td className="py-0.5 pr-3">
                    {nestedSchema ? (
                      <SchemaTree
                        schema={nestedSchema}
                        schemas={schemas}
                        name={
                          value.type === "array" ? `${nestedName}[]` : (nestedName ?? undefined)
                        }
                        depth={depth + 1}
                      />
                    ) : (
                      <Mono>{typeOf(value)}</Mono>
                    )}
                  </td>
                  <td className="py-0.5 text-[12px] text-ink-muted">
                    {value.description}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      ) : null}
    </div>
  );
}

export function ApiDocsEmpty() {
  return <Empty>no document</Empty>;
}
