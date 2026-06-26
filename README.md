# limited-shell

> **Status: coming soon.** The core compiles and is heavily tested; the public surface is still being shaped.

A capability-scoped shell language. Commands are parsed, type-checked, scheduled and executed under explicit resource and policy limits — so an agent can be given exactly the authority it needs and no more.

Part of [**simple tools**](https://zeta1999.github.io/renoir42/simple-tools.html) — small, composable Rust libraries for building tooling fast from a harness.

## Idea

A normal shell trusts whoever runs it. `limited-shell` instead makes authority explicit: every command runs inside a scope that bounds the resources it can touch and the policy it must satisfy. The pipeline is a small language —

```
parse → type-check → schedule → execute
```

— with a cost model and a resource/extent engine in between, so limits are enforced before anything runs, not after.

## Formal models

Key pieces are modelled in **Lean 4** — the role hierarchy and the policy engine among them — so the access-control core can be checked, not just tested.

## Layout

```
limited-core/   parser, type-checker, scheduler, execution engine
lean/           Lean 4 models (role hierarchy, policy engine, ...)
```

See [`DESIGN_REMOTE.md`](DESIGN_REMOTE.md) for the remote-execution layer that [`simple-remote`](https://github.com/zeta1999/simple-remote-public) builds on.

## License

MIT OR Apache-2.0
