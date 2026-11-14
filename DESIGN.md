# Kestrel design system

Kestrel should feel like a Windows 11 utility, not a web dashboard. All tokens and components live in `src/ui/components.rs`; screens never style things by hand.

## Principles
- **Lists, not cards.** Related items sit in one bordered group with dividers (as in Windows Settings and Linear), never a card per item and never cards inside cards.
- **One accent, one primary action per screen.** The muted blue marks the main action, selection and "on" states, and nothing else.
- **Hierarchy from weight and size, not color.** Segoe UI with Semibold (never Bold), plus three text colors: primary, secondary, tertiary.
- **Status is a dot and a word.** Green means fine, amber means worth a look, grey means neutral. No badges, no emoji.
- **Tell people what will happen.** Every action has a one-line description; anything destructive gets a confirmation that names what changes; results arrive as non-blocking notifications.

## Tokens
| Token | Value | Use |
|---|---|---|
| `BG` | #202020 | Page background (Windows 11 dark base) |
| `SIDEBAR` | #1B1B1B | Navigation pane |
| `LAYER` / `LAYER_HOVER` | #2A2A2A / #323232 | List groups, dialogs, notifications / hover |
| `STROKE` / `DIVIDER` | #3A3A3A / #333333 | 1 px borders / row dividers |
| `TEXT` / `TEXT_2` / `TEXT_3` | #EEEEEE / #B0B0B0 / #868686 | Primary / secondary / tertiary text |
| `ACCENT` / `ACCENT_TEXT` | #4C8BE8 / #8DB6F2 | Primary button, selection, on-state / links |
| `SUCCESS` / `CAUTION` / `CRITICAL` | #6CCB5F / #E3B341 / #F1707B | Status only |

**Type** (Segoe UI from `%WINDIR%\Fonts`, Semibold from `seguisb.ttf`, matching the Windows 11 type ramp): Title 20 semibold · Body 14 · Body strong 14 semibold · Caption 12 · Monospace 12.5 (Consolas).

**Spacing:** a 4 px grid (4, 8, 12, 16, 24). Rows are padded 14 × 10; sections are 16–24 px apart.

**Shape:** 4 px corners on controls, 6 px on list groups, 8 px on dialogs and notifications (Windows 11 rounded-corner guidance). Borders rather than shadows; only dialogs and notifications cast a soft shadow.

**Icons:** Segoe Fluent Icons (Windows 11) or Segoe MDL2 Assets (Windows 10) at 16 px, in navigation only.

## Components
| Component | Use |
|---|---|
| `page_header` | Page title plus one line of context |
| `section` | Section heading with optional right-aligned links |
| `group` + `row` | Bordered list with dividers |
| `setting_row` / `setting_row_note` | Title and description left, control right; optional status dot and caution note |
| `table_header` + `cell` | Column layout inside a group (Cleanup, Apps) |
| `accent_button` / `button` / `row_button` / `link` | Primary, standard, compact row action, and low-emphasis text action |
| `toggle` | Windows-style switch with "On"/"Off", for settings that apply immediately |
| `checkbox` | Accent-filled check, for selections applied later (cleanup categories, presets) |
| `pivot` | Tabs with an accent underline (Gaming) |
| `segmented` | Joined choices (presets) |
| `status` / `tag` | Dot plus word / quiet outlined label (Store, Protected) |
| `info_bar` | Inline message with a colored edge and optional action |
| `empty_state` / `progress` | Nothing-yet and loading states inside a group |
| `dialog` / `confirm` | Modal with right-aligned buttons, primary first |
| `toasts` | Bottom-right notifications: success fades after 8 s, errors stay until dismissed |

**Interaction:** every custom control (navigation items, toggles, checkboxes, pivots, buttons) draws a 2 px focus outline when reached with Tab, and Enter or Space activates it. Hover changes the fill, never the size. Disabled controls are dimmed and explain why in a tooltip.

## Information architecture
Overview (main actions, the Game Mode switch, health status) · Cleanup · Apps · Startup apps · Gaming (Windows settings, Per-game) · Game Mode · Settings, pinned at the bottom of the navigation. The health check moved from Gaming to the Overview, so the first screen says whether anything needs attention.
