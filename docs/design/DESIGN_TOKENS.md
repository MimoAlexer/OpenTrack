# Design tokens

The approved product direction is the compact neutral theme implemented by the frontend in [styles.css](../../web/src/styles.css). That source currently owns the working token values. Its charcoal/off-white surfaces, restrained indigo focus/selection and neutral primary buttons supersede the old teal/slate palette below. The frontend owns exact value reconciliation and new contrast/runtime evidence; the Phase 0 contrast result must not be cited as proof for changed colors.

The following tables preserve the historical Phase 0 proposal until the implemented token set receives its own recorded contrast/accessibility review. They are not instructions to restore rejected styling. Use semantic tokens rather than raw component palette values.

| Semantic token | Light | Dark | Use |
| --- | --- | --- | --- |
| canvas | #F8FAFC | #0B1220 | Application background |
| surface | #FFFFFF | #111827 | Panels, dialogs |
| surface-raised | #F1F5F9 | #1E293B | Hover/secondary panels |
| text-primary | #0F172A | #F1F5F9 | Body and headings |
| text-secondary | #475569 | #CBD5E1 | Metadata |
| text-muted | #526176 | #94A3B8 | Tertiary text; still readable |
| border-subtle | #CBD5E1 | #334155 | Decorative dividers only |
| border-control | #64748B | #64748B | Essential input/control boundary |
| accent | #0F766E | #5EEAD4 | Primary fill, link, focus ring |
| accent-hover | #115E59 | #99F6E4 | Primary hover fill |
| accent-soft | #CCFBF1 | #134E4A | Selection background |
| on-accent | #FFFFFF | #042F2E | Text on primary fill |
| success-text | #166534 | #86EFAC | Completed/success label |
| success-surface | #DCFCE7 | #14532D | Success message background |
| warning-text | #92400E | #FDE68A | Pending approval/warning label |
| warning-surface | #FEF3C7 | #78350F | Warning background |
| danger-text | #B91C1C | #FCA5A5 | Destructive/error label |
| danger-surface | #FEE2E2 | #7F1D1D | Error background |
| info-text | #1D4ED8 | #93C5FD | Informational label |
| info-surface | #DBEAFE | #1E3A8A | Information background |

Accent-soft selection uses text-primary or the matching accessible accent foreground, never on-accent (which is for the solid accent fill). Error buttons use danger-text as foreground on danger-surface or an independently tested solid-danger pair. Border-subtle must not be the only indicator of an editable input. Overlay uses black at 40% light/60% dark, with actual modal content tested on opaque surface.

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

Spacing tokens: space-0=0,1=4 px,2=8 px,3=12 px,4=16 px,5=20 px,6=24 px,8=32 px,10=40 px,12=48 px,16=64 px. An explicit 2 px micro-gap is permitted for icon alignment, not general layout. Radii: none 0, small 4 px, medium 6 px, large 8 px, dialog 12 px, pill 999 px. Borders 1 px; focus 2 px+2 px offset. Heights: input/button 36 px, small 32 px, touch 44 px; list compact 32 px/default 40 px/touch 48 px. Minimum independent pointer target 24 px with spacing, preferred 44 px for touch.

Elevation: base none; popover light 0 4 px 12 px rgba(15,23,42,0.10), dark 0 4 px 16 px rgba(0,0,0,0.35); modal light 0 12 px 32 px rgba(15,23,42,0.16), dark 0 16 px 40 px rgba(0,0,0,0.50). Shadows supplement visible boundary, never replace it.

Layer order: base0, sticky 10, sidebar drawer 20, popover 30, modal 40, toast 50, tooltip 60. Modal-owned popovers render inside its focus/stacking boundary. No arbitrary z-index 99999.

Motion: immediate 0 ms, feedback 80 ms, standard 120 ms, panel 160 ms, maximum 200 ms; ease-out cubic-bezier(0.2,0,0,1). Animate opacity or small transform only when useful. Reduced motion: all transforms and skeleton shimmer off, opacity change<=80 ms or immediate. No scroll-jacking or auto-scroll on incoming list updates.

Breakpoints: compact<768 px; intermediate 768–1199 px; desktop>=1200 px. At compact show one main pane and a drawer sidebar; intermediate sidebar collapses to icon rail only if labeled tooltips and keyboard navigation remain complete, otherwise drawer. Detail aside only when main list retains 480 px. These are layout thresholds, not device identity.

Status semantics: triage question icon+text-muted; backlog dotted circle+text-muted; unstarted open circle+info-text; started partial circle+warning-text; completed check-circle+success-text; canceled crossed circle+text-secondary. Priorities: urgent exclamation+danger-text; high two-up chevrons+warning-text; normal horizontal lines+text-secondary; low down-chevron+text-muted; unset dash+text-muted. Always include accessible name.
