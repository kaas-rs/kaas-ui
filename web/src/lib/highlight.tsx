// Syntax colouring for the two things kaas-ui shows source of: a registered
// schema, and a decoded JSON payload.
//
// **Hand-written, and about eighty lines of it.** The same call the line diff
// made: a highlighting library is a large dependency for one screen, and the
// grammars involved are JSON — which is trivial to tokenise correctly — and
// `.proto`, where keywords and strings are the whole of what anyone wants
// coloured. Shiki carries a WASM regex engine; Prism carries a plugin system.
// Neither buys anything here that this file does not.
//
// **The palette is the design system's, not a theme of its own.** Every colour
// below is an existing semantic token, so the result is legible in light and
// dark without a second set of values to keep in step — and a schema does not
// get to introduce colours the rest of the application has never used.
//
//   keys        rust-ink   the accent, and what you scan a schema for
//   strings     ok         the value half of a pair, distinct from its key
//   numbers     warn-ink   including the ids and versions that matter here
//   literals    danger     `true`, `false`, `null` — few, and worth spotting
//   punctuation ink-faint  braces and commas recede; they are structure

import type { ReactNode } from "react"

/** One coloured run. `null` means "no class": ordinary text. */
type Token = [text: string, className: string | null]

const KEY = "text-rust-ink"
const STRING = "text-ok"
const NUMBER = "text-warn-ink"
const LITERAL = "text-danger"
const PUNCTUATION = "text-ink-faint"
const COMMENT = "text-ink-faint italic"

/**
 * Tokenise JSON.
 *
 * A scanner rather than a walk of `JSON.parse`'s output, because the text is
 * what is on screen: pretty-printed, with its own whitespace and ordering, and
 * re-serialising a parse tree would colour a *different document* from the one
 * being read.
 *
 * A string is a key when the next non-space character is a colon. That is the
 * whole rule, and it is exact for JSON — nothing else can precede one.
 */
function json(source: string): Token[] {
  const tokens: Token[] = []
  let index = 0

  while (index < source.length) {
    const rest = source.slice(index)

    const string = /^"(?:[^"\\]|\\.)*"/.exec(rest)
    if (string) {
      const [text] = string
      const after = rest.slice(text.length)
      tokens.push([text, /^\s*:/.test(after) ? KEY : STRING])
      index += text.length
      continue
    }

    const number = /^-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/.exec(rest)
    if (number) {
      tokens.push([number[0], NUMBER])
      index += number[0].length
      continue
    }

    const literal = /^(?:true|false|null)\b/.exec(rest)
    if (literal) {
      tokens.push([literal[0], LITERAL])
      index += literal[0].length
      continue
    }

    const punctuation = /^[{}[\],:]+/.exec(rest)
    if (punctuation) {
      tokens.push([punctuation[0], PUNCTUATION])
      index += punctuation[0].length
      continue
    }

    // Whitespace, and anything this grammar does not know. Uncoloured rather
    // than guessed at: a token nobody recognised is text.
    const plain = /^[^"\-\d{}[\],:tfn]+|^./.exec(rest)
    const text = plain ? plain[0] : rest.slice(0, 1)
    tokens.push([text, null])
    index += text.length
  }

  return tokens
}

/** `.proto` source: comments, strings, numbers, and the keywords. */
const PROTO_KEYWORDS =
  /^(?:syntax|package|import|option|message|enum|service|rpc|returns|repeated|optional|required|reserved|oneof|map|extend|public|weak|stream)\b/

const PROTO_TYPES =
  /^(?:double|float|int32|int64|uint32|uint64|sint32|sint64|fixed32|fixed64|sfixed32|sfixed64|bool|string|bytes)\b/

function proto(source: string): Token[] {
  const tokens: Token[] = []
  let index = 0

  while (index < source.length) {
    const rest = source.slice(index)

    const comment = /^\/\/[^\n]*|^\/\*[\s\S]*?\*\//.exec(rest)
    if (comment) {
      tokens.push([comment[0], COMMENT])
      index += comment[0].length
      continue
    }

    const string = /^"(?:[^"\\]|\\.)*"/.exec(rest)
    if (string) {
      tokens.push([string[0], STRING])
      index += string[0].length
      continue
    }

    const keyword = PROTO_KEYWORDS.exec(rest)
    if (keyword) {
      tokens.push([keyword[0], KEY])
      index += keyword[0].length
      continue
    }

    const type = PROTO_TYPES.exec(rest)
    if (type) {
      tokens.push([type[0], LITERAL])
      index += type[0].length
      continue
    }

    const number = /^\d+/.exec(rest)
    if (number) {
      tokens.push([number[0], NUMBER])
      index += number[0].length
      continue
    }

    const punctuation = /^[{}[\]()<>=;,]+/.exec(rest)
    if (punctuation) {
      tokens.push([punctuation[0], PUNCTUATION])
      index += punctuation[0].length
      continue
    }

    const plain = /^[A-Za-z_][\w.]*|^\s+|^./.exec(rest)
    const text = plain ? plain[0] : rest.slice(0, 1)
    tokens.push([text, null])
    index += text.length
  }

  return tokens
}

/**
 * Colour `source`, as spans, for rendering inside a `<pre>`.
 *
 * Falls back to the text itself on anything it cannot tokenise, so a malformed
 * schema is still *shown*. Colour is a reading aid; refusing to render one
 * because it could not be parsed would be the aid deciding what you may see.
 */
export function highlight(
  source: string,
  language: "json" | "proto"
): ReactNode {
  let tokens: Token[]
  try {
    tokens = language === "json" ? json(source) : proto(source)
  } catch {
    return source
  }

  // A scanner that dropped or duplicated a character would be a renderer that
  // silently rewrites a schema. Cheap to rule out, so it is ruled out.
  if (
    tokens.reduce((total, [text]) => total + text.length, 0) !== source.length
  ) {
    return source
  }

  return tokens.map(([text, className], index) =>
    className ? (
      <span key={index} className={className}>
        {text}
      </span>
    ) : (
      text
    )
  )
}
