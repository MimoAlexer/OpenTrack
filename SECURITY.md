# Security reporting

OpenTrack is under development in a private repository. There is no published supported-release window or response-time commitment. Include the affected commit or package version in a report; implementation and qualification limits are recorded in the [requirements ledger](docs/REQUIREMENTS_STATUS.md) and [current handoff](docs/PROJECT_HANDOFF_2026-10-18.md).

## Arrange a private report

Do not put vulnerability details, exploit steps, credentials, or affected workspace data in an ordinary issue or pull request. A private repository can still have readers who do not need access to a vulnerability report.

Contact the repository maintainer through an established private channel and agree on how to transfer the report. If you have repository access but no private contact, open an issue containing only **Security reporting contact requested** and ask the maintainer to arrange a private channel. Do not include technical details in that request.

No security email address is published here, and this policy does not claim that GitHub private vulnerability reporting is enabled. A dedicated reporting route and support policy still need to be established before a public release.

## What to include privately

- Affected commit/package, operating system, and deployment or client type.
- The impact and required access, including whether another workspace, private team, session, file, or operator capability is involved.
- A minimal reproduction using synthetic data in an instance you control, with expected and observed behavior.
- Redacted logs or request IDs, and any known workaround. Never send live passwords, session cookies, database URLs, setup/invitation/recovery codes, or vault contents.

Use accounts and systems you are authorized to test. Avoid accessing other people's data or running destructive tests on shared instances. Discuss disclosure and any additional testing with the maintainer through the agreed channel; this document does not establish a bounty or legal safe-harbor program.

For the implementation's trust boundaries and threat register, see [security architecture](docs/SECURITY_ARCHITECTURE.md). For local session revocation, operator recovery, and account disablement, see [account security](docs/ACCOUNT_SECURITY.md). Those operational controls do not substitute for investigating or fixing a vulnerability.
