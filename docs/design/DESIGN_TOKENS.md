# Design tokens

The approved product direction is the compact neutral theme implemented in [styles.css](../../web/src/styles.css): charcoal/off-white surfaces, restrained indigo focus/selection and neutral primary buttons. The semantic color table below matches that implementation. [Shared-primitives evidence](../evidence/2026-10-22-design-primitives.md) records computed browser contrast and exercised application controls; historical Phase 0 results do not qualify changed values.

Use semantic CSS tokens rather than raw component palette values.

| CSS token | Light | Dark | Use |
| --- | --- | --- | --- |
| bg | #FCFCFB | #1B1B1E | Canvas and modal background |
| side | #F3F3F1 | #161618 | Navigation and group headings |
| panel | #FFFFFF | #202023 | Inputs and popovers |
| hover | #F0F0EE | #252528 | Hover and active option surface |
| selected | #EBECF1 | #2A2A32 | Selected option/navigation surface |
| text | #242426 | #E9E9EC | Primary text |
| secondary | #606067 | #AAAAB2 | Secondary text |
| muted | #66666E | #9797A1 | Readable metadata/placeholders |
| line | #E2E2DF | #333337 | Decorative dividers only |
| control | #73737C | #73737C | Essential control and popover boundary |
| accent | #4D5BB7 | #A5ADF0 | Links, focus and active option outline |
| accent-bg | #EDF0FA | #2C3042 | Informational/selection surface |
| green | #416F50 | #95B6A0 | Success text |
| green-bg | #EDF3EE | #27322C | Success surface |
| amber | #865E13 | #CFB484 | Warning text |
| amber-bg | #F7F0DF | #382F23 | Warning surface |
| red | #A13C36 | #EEAAA4 | Error/destructive text |
| red-bg | #FCF0EF | #3C2827 | Error surface |
| solid | #2B2B2E | #DEDEE4 | Primary button fill |
| solid-text | #FFFFFF | #252528 | Primary button text |

Selection uses `text`, never `solid-text`. Error text uses `red` on `red-bg`. Decorative `line` must not be the sole boundary of an editable field or picker. The shared keyboard focus outline is 2px `accent` with a 3px offset; an active combobox option uses a 2px inset outline as well as a surface change. Disabled controls retain their native disabled state; explanatory text stays readable.

| Typography token | Size / line height | Weight | Purpose |
| --- | --- | --- | --- |
| caption | 12 / 16 px | 400–500 | Secondary timestamps, never essential action labels |
| body | 14 / 20 px | 400 | Dense work UI |
| body-strong | 14 / 20 px | 500 | Row title and button |
| body-large | 16 / 24 px | 400 | Reading/editor content |
| section | 16 / 24 px | 600 | Panel section |
| issue-title | 20 / 28 px | 600 | Detail title |
| page-title | 24 / 32 px | 600 | Main page |
| display | 32 / 40 px | 600 | Onboarding only |
| code | 13 / 20 px | 400 | Monospace blocks/logs |

Spacing tokens: space-0=0,1=4 px,2=8 px,3=12 px,4=16 px,5=20 px,6=24 px,8=32 px,10=40 px,12=48 px,16=64 px. An explicit 2 px micro-gap is permitted for icon alignment, not general layout. Radii: none 0, small 4 px, medium 6 px, large 8 px, dialog 12 px, pill 999 px. Borders 1 px; focus 2 px+3 px offset. Heights: input/button 36 px, small 32 px, touch 44 px; list compact 32 px/default 40 px/touch 48 px. Minimum independent pointer target 24 px with spacing, preferred 44 px for touch.

Elevation: base none; popover light 0 4 px 12 px rgba(15,23,42,0.10), dark 0 4 px 16 px rgba(0,0,0,0.35); modal light 0 12 px 32 px rgba(15,23,42,0.16), dark 0 16 px 40 px rgba(0,0,0,0.50). Shadows supplement visible boundary, never replace it.

Layer order: base0, sticky 10, sidebar drawer 20, popover 30, modal 40, toast 50, tooltip 60. Modal-owned popovers render inside its focus/stacking boundary. No arbitrary z-index 99999.

Motion: immediate 0 ms, feedback 80 ms, standard 120 ms, panel 160 ms, maximum 200 ms; ease-out cubic-bezier(0.2,0,0,1). Animate opacity or small transform only when useful. Reduced motion: all transforms and skeleton shimmer off, opacity change<=80 ms or immediate. No scroll-jacking or auto-scroll on incoming list updates.

Breakpoints: compact<768 px; intermediate 768–1199 px; desktop>=1200 px. At compact show one main pane and a drawer sidebar; intermediate sidebar collapses to icon rail only if labeled tooltips and keyboard navigation remain complete, otherwise drawer. Detail aside only when main list retains 480 px. These are layout thresholds, not device identity.

Status semantics: triage question icon+text-muted; backlog dotted circle+text-muted; unstarted open circle+info-text; started partial circle+warning-text; completed check-circle+success-text; canceled crossed circle+text-secondary. Priorities: urgent exclamation+danger-text; high two-up chevrons+warning-text; normal horizontal lines+text-secondary; low down-chevron+text-muted; unset dash+text-muted. Always include accessible name.
