# Upload scope and project-update comparison: focused qualification

Date: 28 October 2026. This record covers the compatible F13 and X17 increments and their interaction with project-default issue creation. It does not close an issue or establish complete feature acceptance.

- The attachment candidate `fc64fca` passed 22 upload-hook and 6 editor tests plus both TypeScript targets. The project-update candidate `67ee899` passed 41 project tests and both TypeScript targets. Their source files were unchanged when combined.
- Independent review found that project-access denial could unmount the editor and lose a pending upload tracker. The correction retains the editor and upload state while concealing its contents and closing nested dialogs; focused tests covered reservation recovery and real-editor concealment.
- The combined frontend candidate `8e7cb13` passed 892 tests across 82 files, both TypeScript targets and a production build. Browser discovery listed 258 cases; those cases were not executed in this local run.
- The earlier full F23 frontend run and its two corrected fixture failures are recorded in [template project verification](2026-10-28-template-project-defaults.md).
- Installed native behavior, physical input, assistive technology and the final hosted browser matrix remain unverified. Project-update comparisons still exclude dates, risks and decisions; attachment recovery and storage qualification remain broader work.
