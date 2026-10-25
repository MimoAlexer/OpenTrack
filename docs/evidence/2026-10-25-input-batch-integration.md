# Input and picker batch integration

25 October2026. PR166 merged as `8cd9f0879750a6658f78822ff671aee1ca603913`
after all four checks passed at the reviewed head `7369ae4`. The
[receipt](2026-10-25-input-batch-integration.json) binds those checks, actual browser
results, review and integration readback.

The AMD64 browser job passed213/213 saved-workspace cases:71 each in Chromium,
Firefox and WebKit. All213 browser/title pairs are unique. Both the successful
end message and actual JSON receipt account for every case. The
[runtime run](https://github.com/MimoAlexer/OpenTrack/actions/runs/40229959618)
and [implementation/native run](https://github.com/MimoAlexer/OpenTrack/actions/runs/40229959704)
qualify this head. The merge tree equals the reviewed tree; remote main, merge
parents, neutral identity, zero unresolved conversations and the integration lock
were verified. Main still has26 migrations.

The explicitly approved900-second whole-suite limit preserves per-test60seconds,
expect/action10seconds, one worker, zero retries and assertions. The earlier
191-pass/two-failure and201-pass incomplete600-second runs remain retained with
their original logs. The separate three fresh-setup cases retain their own proof.

This completes this input/CSS batch's integration gates. It does not qualify the
private manual-order implementation, OS IME, native GUI, physical-input,
assistive-technology, whole-feature or complete-product acceptance. Post-merge CI
is a separate run and is not claimed by this record.
