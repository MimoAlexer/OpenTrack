# Design system

OpenTrack's interface should feel composed, legible and purposeful. The approved visual direction uses compact charcoal/off-white neutral surfaces, restrained indigo for selection/focus, strong typography and predictable alignment. This supersedes the earlier teal/slate exploration. No Linear assets, logo, proprietary component code or copied brand palette are used. [Tokens](DESIGN_TOKENS.md) are canonical; [components](COMPONENT_SPECIFICATIONS.md) and [screens](SCREEN_SPECIFICATIONS.md) apply them.

## Information hierarchy

The issue title is the primary row content; status, key, assignee and priority are secondary; labels and dates are optional tertiary information. A dense issue row is not a miniature form. Reveal secondary actions on focus as well as hover, and keep an overflow control permanently discoverable. Use one filled primary button per dialog/task region; destructive actions use explicit wording and the danger treatment.

Workspace and team are context, not decorations. Show current workspace in the switcher and current team/project in the page breadcrumb. A project overview communicates purpose, health, dates and scope before offering charts. The agent screen communicates permission, current state, cost limits and review evidence before model identity.

## Layout and density

Desktop reference viewport 1440×900. Sidebar 240 px, adjustable 200–320 px; app header 48 px; content toolbar 48 px; default content gutters 24 px. Issue detail aside 480–640 px, minimum list area 480 px; if both cannot fit, navigate to full-page detail. Description reading width maximum 760 px. Settings forms maximum 720 px; full tables may exceed that. Never use fixed viewport heights that clip zoomed content.

Comfortable row height 40 px, compact 32 px, touch 48 px. Compact mode preserves 14 px text and at least 24×24 px action targets with spacing; it must not shrink checkboxes to unreadable targets. List density is a user preference, not a workspace override. At 200% zoom switch to responsive layout rather than squeeze columns.

## Typography and icons

Use system sans stack: system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif; code uses ui-monospace, SFMono-Regular, Consolas, monospace. No runtime font fetch. Set body 14/20 px, prominent body 16/24 px, page title 24/32 px and issue title 20/28 px. Medium 500 for labels, semibold 600 for headings; avoid thin typography. Numbers in tables use tabular figures; prose wraps naturally. Use Lucide line icons at 16/20 px with 1.75–2 px stroke, consistent by size; icons supplement names and require accessible labels when standalone.

## Themes and semantics

Light and dark themes share semantic token names; default follows OS, with explicit user choice persisted. Restrained indigo indicates selection and focus; neutral filled buttons indicate primary actions; status colors communicate lifecycle with icons/text and do not borrow red solely for urgency. A destructive state always has a textual reason. No gradient panels, glass effects or pulsing activity surfaces. Neutral borders and a narrow elevation scale separate layers without visual clutter.

Selection, keyboard focus and hover are independent states. A selected row remains distinguishable when focus moves elsewhere. Focus uses a2 px ring plus 2 px offset. Disabled controls retain readable labels with explanation; do not express disabled state only through low opacity. Theme transitions do not animate large color changes.

## Content and feedback

Use concrete verbs: Create issue, Save changes, Retry upload, Cancel run, Review patch. Empty state names why it is empty and one relevant next action. Do not claim a job is complete because the model stopped writing. Error text says what failed, what remains saved and what action is available. Use absolute timestamps on demand and relative times for recent activity; dates always use user locale with team timezone context for planning.

Agent connection settings must name billing/auth mode clearly: OpenAI API key, eligible ChatGPT plan connection, Anthropic API key, local inference. Branding rules from providers constrain labels and logos; the product's overall identity remains OpenTrack.

## Quality review

A screen is ready only when normal/loading/empty/error/permission-denied/offline/conflict states are designed, keyboard behavior is documented, both themes meet contrast, and a large dataset remains navigable. UI performance budgets live in PERFORMANCE_STRATEGY.md. Review actual screenshots and manual interactions in implementation; Phase 0 wireframes are layout specifications, not a validated running interface.

## Approved implementation alignment

The user-approved compact neutral direction takes priority over historical Phase 0 geometry and palette examples. The frontend owns the current token implementation in [styles.css](../../web/src/styles.css), including light/dark, selected/hover/control and semantic states. Preserve the earlier documented details as historical constraints to evaluate, then reconcile exact token values and component dimensions with measured contrast, keyboard targets and zoom/reflow evidence. A stylesheet or screenshot alone does not establish those acceptance checks.


## Current issue-workspace implementation

The18 October refinement uses a220px sidebar,52px header and42px issue rows. Split view starts at1280px, reserving400px list and560px detail widths. Issue content is centered within760px with a24px wrapping title; properties use three columns or two when the actual pane is560px or narrower. Workspace actions live in one bounded menu; empty connections use one action disclosure. Native and web share this layout. These working dimensions refine the historical reference geometry above. See [runtime evidence](../evidence/2026-10-18-ui-refinement.md) for tested interactions and remaining viewport/accessibility limits.
