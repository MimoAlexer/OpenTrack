# Complete Compose preflight inspection

One-line correction over `af778de6dbab2f78cd0d0997cc8fc878ad6fa892`: only read-only `config` selects `--profile *`. Runtime `up`/`run` retain empty profiles and no wildcard; strict role/resource validation remains.

The prior invocation could receive a profile-filtered model without the inactive roles or eighth declared volume. This follows the official [Compose project selection](https://github.com/docker/compose/blob/v2.39.4/cmd/compose/compose.go#L313-L347) and [profile/resource filtering](https://github.com/compose-spec/compose-go/blob/v2.9.0/types/project.go#L341-L442). The installed CI version has not been inspected.

The new mocked-projection case first recorded one expected preflight error. After correction, **51 pure tests passed**, including all previous50. The regression verifies complete metadata inspection and unchanged runtime arguments/environment. Original output and source hashes are in the JSON; all older proof files remain unchanged. `git diff --check` passed.

No Docker, real fixtures, browsers or CI ran. Exact-head review and configured runtime acceptance remain pending. All timing, authority, claim/lifecycle, SQL, Node and profile boundaries remain intact.
