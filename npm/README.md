# stepcheck (npm wrapper)

A static verifier for AWS Step Functions / Amazon States Language (ASL) workflows.
Installing this package downloads the self-contained native `stepcheck` binary for
your platform and exposes it as the `stepcheck` command, so a Node-centric CI can
add it with one line.

```bash
npm install -g @nimrod-f/stepcheck
stepcheck check workflow.asl.json --infer
```

(The package is scoped as `@nimrod-f/stepcheck`; the installed command is still
`stepcheck`.)

Use it as a build gate: `stepcheck` exits `0` (clean), `1` (errors), or `2`
(parse failure); add `--deny-warnings` to fail on warnings once a workflow is
annotated. See the project README for the full command set.

The binary is fetched from the matching GitHub release. To point at a different
release repository, set `STEPCHECK_REPO=owner/repo` before install. If no prebuilt
binary matches your platform, install from source with `cargo install stepcheck`.
