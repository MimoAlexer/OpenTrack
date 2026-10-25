# Private feature integration preparation

25 October 2026. [Receipt](2026-10-25-manual-feature-union.json), checked head
`6ba9482fa5c7b86b0a5b4f4017554496dafa960b`.

The private manual-order branch now includes the previously reviewed input/CSS
batch, palette team/project discovery, reviewed document append/replace and rich
Markdown portability, and planning write recovery with its review corrections.
The historical rank-fixture correction is also imported; its independent review
remains pending. Original source-specific success and failure records are retained.

The combined source passes 164 focused tests across nine files and both application
and browser TypeScript checks. Thirteen checked production files exactly match
their reviewed candidates. Milestones is the exact reviewed recovery source with
the Unicode title correction already present on main retained. Backend production
code remains byte-identical to the corrected `ab00bf4` source, so its frozen server
artifact remains valid without another identical build.

This is preparation for the connected manual-order UI union. No current combined
production build or browser matrix was executed. Those checks, independent review
and all four exact-head hosted gates remain required before integration. The
public PR166 matrix remains incomplete at 201 passes and 12 unreported cases;
privately composing its source does not clear that gate. No timeout, profile or
assertion changed, and no issue, milestone or whole-product completion is claimed.
