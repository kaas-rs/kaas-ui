# Design system

kaas-ui uses **mdbook's `rust` theme palette** — the same colours as the kaas-lib
book, which sets `default-theme = "rust"`. Pointing the two at each other should
feel like one project, not two.

The brief is *modern application, mdbook colours*: warm paper ground, rust
accent, generous whitespace, crisp type, flat surfaces separated by hairlines
rather than shadows, and a real dark mode. No gradients, no glassmorphism, no
decorative colour. In a tool whose whole job is telling you which partition is
offline, colour is a signal and spending it on decoration is spending it twice.

## The palette

Light mode is mdbook's `.rust` block, verbatim. These are the source values:

| mdbook var | value | |
|---|---|---|
| `--bg` | `hsl(60, 9%, 87%)` → `#E1E1DB` | warm paper |
| `--fg` | `#262625` | near-black, slightly warm |
| `--sidebar-bg` | `#3B2E2A` | dark brown |
| `--sidebar-fg` | `#C8C9DB` | cool grey-lilac |
| `--sidebar-active` | `#E69F67` | **the rust orange** |
| `--sidebar-spacer` | `#45373A` | |
| `--links` | `#2B79A2` | blue |
| `--inline-code-color` | `#6E6B5E` | olive-grey |
| `--theme-popup-bg` | `#E1E1DB` | |
| `--theme-popup-border` | `#B38F6B` | tan |
| `--theme-hover` | `#99908A` | |
| `--quote-bg` | `hsl(60, 5%, 75%)` → `#C2C2BC` | |
| `--quote-border` | `hsl(60, 5%, 70%)` → `#B6B6AF` | |
| `--warning-border` | `#FF8E00` | amber |
| `--table-border-color` | `hsl(60, 9%, 82%)` → `#D5D5CD` | |
| `--table-header-bg` | `#B3A497` | |
| `--table-alternate-bg` | `hsl(60, 9%, 84%)` → `#DADAD3` | |
| `--search-mark-bg` | `#E69F67` | |
| `--footnote-highlight` | `#D3A17A` | |

Dark mode is **derived**, not borrowed: mdbook's `rust` theme is light-only, and
its dark themes (`coal`, `navy`, `ayu`) are cool-toned and would read as a
different product. So dark mode inverts the paper while keeping the identity —
the sidebar browns become the surfaces, and `#E69F67` stays the accent.

## Semantic tokens

Components reference **these**, never the raw hexes. Tailwind 4 configures in
CSS, so this is the whole theme definition:

```css
@import "tailwindcss";

@theme {
  /* Ground and ink */
  --color-surface:        #E1E1DB;   /* page */
  --color-surface-raised: #EAEAE4;   /* cards, panels — lifted, not shadowed */
  --color-surface-sunken: #DADAD3;   /* table zebra, code blocks */
  --color-surface-nav:    #3B2E2A;   /* sidebar / header — dark in BOTH modes */
  --color-ink:            #262625;
  --color-ink-muted:      #6E6B5E;
  --color-ink-faint:      #99908A;
  --color-ink-on-nav:     #C8C9DB;

  /* Lines */
  --color-line:           #D5D5CD;   /* hairline between rows, cards, cells */
  --color-line-strong:    #B6B6AF;   /* section dividers, table outer edge */
  --color-line-nav:       #45373A;

  /* Accent — rust orange. See "Where accent may and may not go". */
  --color-accent:         #E69F67;
  --color-accent-edge:    #B38F6B;
  --color-accent-ink:     #8F5A2B;   /* accent as TEXT on light ground */

  /* Link */
  --color-link:           #2B79A2;

  /* Status. Not in mdbook — a Kafka UI needs them; tuned to the warm ground. */
  --color-ok:             #3F6431;
  --color-ok-soft:        #DDE4D6;
  --color-warn:           #FF8E00;   /* mdbook's own --warning-border */
  --color-warn-ink:       #8A5200;
  --color-warn-soft:      #F5E3C8;
  --color-danger:         #A33A2B;
  --color-danger-soft:    #EFD8D3;

  /* Type */
  --font-sans: "Inter var", ui-sans-serif, system-ui, -apple-system, sans-serif;
  --font-mono: "JetBrains Mono", ui-monospace, "SF Mono", Menlo, monospace;

  /* Geometry — square-ish, like the book */
  --radius-sm: 3px;
  --radius:    5px;
  --radius-lg: 8px;
}

@media (prefers-color-scheme: dark) {
  :root {
    --color-surface:        #241D1A;
    --color-surface-raised: #2E2521;
    --color-surface-sunken: #1C1613;
    --color-surface-nav:    #3B2E2A;
    --color-ink:            #E8E6DF;
    --color-ink-muted:      #A9A296;
    --color-ink-faint:      #7D746C;
    --color-line:           #3F332E;
    --color-line-strong:    #52433C;
    --color-accent-ink:     #E69F67;   /* on dark, the accent IS legible as text */
    --color-link:           #7AB8D9;
    --color-ok:             #8FBF7A;
    --color-ok-soft:        #2A3524;
    --color-warn-ink:       #F0A94C;
    --color-warn-soft:      #3A2C15;
    --color-danger:         #E08272;
    --color-danger-soft:    #3A211C;
  }
}
```

`:root[data-theme="dark"]` and `[data-theme="light"]` override the media query,
because the header carries a theme toggle — the same affordance mdbook has, and
users will look for it.

### Where accent may and may not go

`#E69F67` on `#E1E1DB` is roughly 2:1. **It is not a text colour on light
ground.** It is a *surface* colour: the active nav item, the focus ring, the
selected-row edge, the search highlight, the 2px rule under an active tab.

For accent-coloured **text** on light, use `--color-accent-ink` (`#8F5A2B`,
≈4.9:1, passes AA). In dark mode the two collapse into one because `#E69F67` on
`#241D1A` is ≈7:1.

This rule is the single easiest thing to get wrong, and getting it wrong makes
the whole UI look washed out.

## Layout

```
┌──────────────────────────────────────────────────────────────┐
│ nav  kaas-ui   [● kaas]  fleet · topics · groups   ⌘K   ☾    │  --color-surface-nav
├──────────┬───────────────────────────────────────────────────┤
│ sidebar  │  content                                          │
│ (nav bg) │  --color-surface, max-w-[1400px], px-8            │
│          │                                                   │
└──────────┴───────────────────────────────────────────────────┘
```

The dark nav band is the strongest visual anchor and it is dark in both modes —
same as the book's sidebar. It is also where the **cluster chip** lives, which
PLAN.md §7 requires to be answerable without reading the URL.

Content is capped at 1400px and left-aligned, not centred: tables of partitions
want the horizontal room, and a centred column on a 4K monitor wastes it.

## Component vocabulary

Built from shadcn/ui primitives, restyled to the tokens above — the components
live in `web/src/components/ui/`, generated by the shadcn CLI and then given
this palette rather than its default grey. `web/src/components/domain.tsx`
holds the ones specific to this product.

**One rename against this document.** The brand orange is the token `rust`, not
`accent`: shadcn spends `--accent` on *hover surfaces*, and two meanings for one
name would put a bright orange behind every dropdown item anyone moused over.
So it is `--rust`, `--rust-edge` and `--rust-ink`, with `--accent` left to
shadcn. Everything else below is unchanged.

Note also that `@theme inline` does not emit `--color-*` as CSS variables — it
substitutes them into the generated utilities. Utilities say `bg-rust`; inline
`style` attributes say `var(--rust)`.

The ones specific to this product:

**`ClusterChip`** — the cluster's colour, id and kind, always in the header.
Colour is derived deterministically from the cluster id by hashing into a fixed
warm ramp (tans, olives, terracottas — all siblings of the palette, never a
random hue). `env: prod` overrides the derived colour with
`--color-danger-soft` and a `--color-danger` edge: prod must not look like
anything else.

**`CapabilityTab`** — a tab that knows it might not exist. Three states:
available (normal), unsupported (**not rendered**), and unsupported-but-routed
(rendered as the panel below when the URL points at it directly).

**`UnsupportedApiPanel`** — the degradation component. Shows the api name and
*both* version ranges, laid out as a comparison rather than prose, because the
pair is the diagnosis:

```
  DescribeTransactions                              api key 65
  ──────────────────────────────────────────────────────────────
  this cluster    does not implement it
  kaas-ui speaks  v0 – v1
```

**`UnknownCodeChip`** — `ErrorCode::Unknown(30000)` rendered as the number in
mono on `--color-warn-soft`, with the number selectable. The number is the only
searchable thing when the broker is newer than the codec.

**`PartitionGrid`** — partitions down, brokers across. Cells: leader
(`--color-accent` fill), follower in-sync (`--color-ok-soft`), out-of-sync
(`--color-warn-soft`), offline (`--color-danger-soft`). Four states, four fills,
no legend needed after five seconds of looking.

**`LagCell`** — "no commit yet", "empty partition" and "zero lag" are three
different states and must not all render as `0`. `—`, `∅` and `0` respectively,
each with a tooltip.

**`RecordRow`** — virtualised, mono, key/value/headers, with the chosen
deserializer shown as a chip that is also the override control. Auto-detection
that cannot be corrected is worse than none.

**`SnapshotAge`** — "as of 4s ago", ticking. On every screen backed by a
snapshot. Turns `--color-warn-ink` past the configured staleness.

**`HintHead` / `SortableHead` / `Stat`'s `hint`** — **every column header and
every stat label carries one line saying what it means.** Not decoration: each
is a Kafka or registry term with a precise meaning and a plausible wrong
reading, and getting it wrong is a wrong operational conclusion. `messages` is
what is *retained*, not what was ever written; `epoch` counts leadership
changes; a subject's `id` is a registry-wide counter and not that subject's
numbering; `compatibility` is a rule about the *next* version rather than a
verdict on this one. One line on hover beats a legend nobody scrolls to, and
beats a footnote for a reader who already knows.

The affordance is one thing learned once: a dotted underline, hover, one
sentence. `HintHead` where the header is a label and `SortableHead` where it is
also the sort control — two components rather than one with an optional
`onClick`, because the element differs (`<span>` vs `<button>`) and a header
that looks clickable and is not is worse than either.

## Type and density

| | |
|---|---|
| body | 14px / 1.5, `--font-sans` |
| table cell | 13px / 1.4 |
| ids, offsets, hex, config keys, version ranges | `--font-mono`, 13px, `--color-ink-muted` |
| section heading | 15px, 600 weight, letter-spacing -0.01em |
| page title | 22px, 600 |

Everything a broker said verbatim is mono. Everything kaas-ui wrote is sans.
That split is doing real work: it tells the reader at a glance which strings
they can paste into `kafka-configs.sh`.

Row height 36px in tables, 32px in dense mode. A twelve-cluster fleet view and a
5000-topic list are both real, so density is a first-class setting, not an
afterthought.

## Icons

lucide for everything generic, and a brand mark where the thing being pointed
at has one of its own: Apache Kafka for a cluster, MQTT for an MQTT broker, a
shelf of books for a schema registry — the last from Remix Icon, a line weight
that sits beside lucide rather than against it. The marks come from
`react-icons`, whose per-icon imports tree-shake: three of them cost ~2.6 kB
gzipped, which is the budget a brand glyph has to earn.

**One table decides, in `components/domain/resource-kinds.ts`.** The nav and the
fleet read it, so a registry cannot be a shelf on one screen and a cylinder on
the other, and the glyph that means *cluster* is never quietly reused for a
resource under one. Icons carry no `size` prop: react-icons default to `1em`
and lucide to 24, and a `size-4` in the class list settles both.

## Motion

Transitions on colour and opacity only, 120ms, `ease-out`. No layout animation:
rows appearing in a live tail must not slide, because a moving target is a
target you cannot click. `prefers-reduced-motion` removes all of it.

## Accessibility floor

- All text ≥ 4.5:1 against its ground; the tokens above are chosen for it.
- **No state is signalled by colour alone.** Offline is a red dot *and* the word
  `unreachable`; under-replicated is amber *and* a count. A red/green fleet
  dashboard is useless to ~8% of men looking at it. The word is what discharges
  this on a status badge, which is why the tick and the cross that used to sit
  between the dot and it are gone: three renderings of one fact, and only one of
  them says which fact.
- Focus ring is `--color-accent`, 2px, offset 2px, never suppressed.
- Every table is keyboard-navigable and every icon-only control is labelled.
