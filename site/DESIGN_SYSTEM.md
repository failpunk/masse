# Masse site design system

The contract. Build only from these names. If a value is needed that is not here, add it
here first, with a name, then use it.

## Thesis

A precision instrument for someone who lives in three inboxes. Swiss timetable and studio
hardware, not SaaS. The page is a spec sheet: the measurements are the argument.

## Bold move

Typographic scale carries the whole page. One enormous statement, one accurate CSS replica
of the app's own chrome, and almost nothing else. No cards, no gradients, no screenshots.

## Type

Display is Archivo 900 at negative tracking. Body is Space Grotesk. Every figure is
JetBrains Mono with tabular numerals, so numbers align in a column like an instrument.
Deliberately not Inter, which is the default and reads as templated.

| Token | Value |
|---|---|
| `--font-display` | Archivo, 900 |
| `--font-body` | Space Grotesk, 400 / 500 |
| `--font-mono` | JetBrains Mono, 400 / 500, `tabular-nums` |
| `--step-hero` | `clamp(44px, 9vw, 122px)` / line-height .88 / tracking -.035em |
| `--step-lead` | `clamp(17px, 2vw, 21px)` / line-height 1.5 |
| `--step-body` | 16px / 1.55 |
| `--step-figure` | 40px / 1 (mono) |
| `--step-micro` | 10px / tracking .18em / uppercase |

## Colour roles

Three inks and one accent. No second accent, no gradient. Cool grey ground, never cream,
and no warm accent, so it carries no resemblance to any other brand.

| Token | Value | Role |
|---|---|---|
| `--ink` | `#0b0d0f` | text, rules, the dark figure |
| `--ink-soft` | `#5b6169` | secondary text, labels |
| `--paper` | `#f1f2f4` | page ground |
| `--paper-lift` | `#ffffff` | the one raised surface (the figure) |
| `--accent` | `#1b4dff` | one accent: the download, one numeral, the active tab |

## Spacing

4px base. Only these: `--s1` 4, `--s2` 8, `--s3` 12, `--s4` 16, `--s5` 24, `--s6` 32,
`--s7` 48, `--s8` 72, `--s9` 112.

## Rules, radii, motion

| Token | Value |
|---|---|
| `--rule` | `1px solid #0b0d0f` (hairline, full-bleed section dividers) |
| `--rule-soft` | `1px solid #d3d6da` |
| `--radius` | `0` everywhere except the account circles, which are `50%` |
| `--ease` | `cubic-bezier(.22,1,.36,1)` |
| `--dur` | `160ms` |

No shadows. A raised surface is expressed with a rule, not a blur.

## Breakpoints

One: `760px`. Below it the grid collapses to a single column and the hero step shrinks by
its own clamp.

## Voice

Sentence case. Second person. No em dashes. No emoji. No hype verbs. Numbers only if
measured. Never name or imply a competing product.
