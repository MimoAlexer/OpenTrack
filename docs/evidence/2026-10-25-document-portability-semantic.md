# Reviewed append and saved Markdown semantics

25 October2026. Parent `e5d064e8405ef2b8eefc9ed41f0e2f5459e0da97`; backend
remains pinned8b. Released editor files had no accepted-main delta before this
work. The [JSON record](2026-10-25-document-portability-semantic.json) binds the
new source, independently specified oracle, executed checks and private index.

After import review, users choose **Replace current draft** or **Append to draft**.
Append keeps current draft blocks and adds reviewed imported blocks at the end.
It reuses canonical size/depth/schema validation and the existing real-editor
normalization/actual-state checks. Rejected application retains the review/draft.
The transfer dialog calls no API or save callback; normal save/autosave and explicit
conflict-review behavior remain unchanged. No converter, RichTextEditor callback,
IssueDocument/comments, App/router, Rust/API/type/schema or shared fixture changed.

Two new API-backed journeys per engine pass **6/6** in Chromium, Firefox and WebKit:

- A handwritten Markdown fixture is paired with an independent expected AST,
  rather than obtaining its oracle from the importer/exporter. Review leaves the
  original saved document untouched; confirmed replace and normal save increment
  the version once. Reload, Markdown export, reviewed reimport and another reload
  preserve paragraph text, headings1/2/3, bold/italic/strike, inline code, exact safe
  href, ordered start3 and nested bullets, ordinary bullets, checked/unchecked
  tasks, two-paragraph quote, hard break and fenced Rust code. Test normalization
  only validates canonical structure, sorts mark order and joins adjacent text
  with identical marks; it does not erase supported structural semantics.
- Unsafe-link review cannot offer append or alter the unsaved draft. Reviewed
  native append retains unsaved text and authorized reference IDs. A normal prior
  draft save is held while a real remote API write commits; the released request
  gets412. The latest appended draft survives, saving is blocked until explicit
  keep-for-review, and normal save/reload persist the combined body with the
  expected version. Native scope and Markdown identity-loss notices remain visible;
  exported Markdown omits reference identities. No new reference/attachment
  authority is fabricated.

Focused changed-dialog/real-editor tests pass **14/14** across two files, including
one undoable append without save, combined-size rejection and permission loss.
Both TypeScript targets and one complete application build pass. After a test-only
correction, the final e2e target passes again; app source/units/build were unchanged.

The initial run's three append/412 cases passed, as did corpus equality through
reimport. Its corpus cases then waited for a save button absent for an unchanged
document. That run remains retained; only the test action changed to the existing
normal save shortcut. Final6/6 results supersede no historical failure and use no
raised timeout, retries or weakened semantic assertion. Existing global transfer,
HTML/paste and physical-input cases were not rerun as a substitute for new proof.

Exact new artifacts are indexed at
`$OPENTRACK_DEV_STATE/document-portability-semantic/artifact-index.json`.
The candidate bundle/maps are frozen under `frontends/candidate`; initial and final
phase roots are `candidate` and `final-candidate`. Both scopes verified empty
browser namespaces, exact API reaping and no cleanup failures. Output drains are
bounded2MiB, files/directories0600/0700, and own database is stopped with all data
retained; named shutdown audit/raw logs are indexed. Node has no PG/database/service
credentials. All prior branches, setup fixtures, datasets, accepted artifacts and
failures remain retained without repeated audits or rebuilds.

No durable migration is required; reverting the UI choice leaves canonical saved
documents valid. Physical keyboard/OS IME, assistive technology, zoom, native
webview, forced cancellation and broader product qualification remain separate.
F12-A3 is not closed. Existing hostile HTML/unknown-schema behavior is unchanged,
with its earlier evidence retained rather than newly claimed here. Independent
review, exact-head hosted checks and integration remain lead-owned. No raw fixture
IDs, credential/session bodies, screenshots, video or traces are committed.
