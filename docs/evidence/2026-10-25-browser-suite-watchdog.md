# Approved browser-suite watchdog adjustment

25 October2026. The human explicitly approved increasing the entire saved-browser
suite watchdog from600 to900 seconds. This is the sole exception to the previous
no-timeout-relaxation instruction. Per-test60seconds, expect/action10seconds, one
worker, zero retries, assertions and case selection remain unchanged.

The executable delta is one subprocess timeout keyword value. Normalized AST
comparison and the three existing pure runtime-boundary tests pass without
starting services. The [receipt](2026-10-25-browser-suite-watchdog.json) binds the
exact driver, unchanged browser configuration and source/test trees.

The original191-pass/two-failure run and the corrected201-pass run remain
incomplete600-second results, with20 and12 unreported cases respectively. The
corrected run passed all three project workflows; it had no reported test failure
and no final end receipt. Neither run qualifies the new head. All four fresh
checks, all213 saved-case results, review/privacy and exact-head integration gates
remain required. This change does not establish application or issue completion.
