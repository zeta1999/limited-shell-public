<p align="center">
  <img src="assets/logo.svg" alt="limited-shell" width="150"/>
</p>

<h1 align="center">limited-shell</h1>

<p align="center">
  <strong>A capability-scoped shell language — give an agent exactly the authority it needs, and no more.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/part%20of-simple%20tools-00d4ff.svg" alt="part of simple tools">
  <img src="https://img.shields.io/badge/Rust-2021-orange.svg?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/status-coming%20soon-yellow.svg" alt="coming soon">
  <img src="https://img.shields.io/badge/model-Lean%204-blueviolet.svg" alt="Lean 4">
  <img src="https://img.shields.io/badge/auth-capability--scoped-008080.svg" alt="capability-scoped">
  <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="license">
</p>

> **⚠ Coming soon.** The core compiles and is heavily tested; the public surface is still being shaped.

> Part of [**simple tools**](https://zeta1999.github.io/renoir42/simple-tools.html) — small, composable Rust libraries for building tooling fast from a harness.

---

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
