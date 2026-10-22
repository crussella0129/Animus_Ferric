# Sprint 128 — Critique

- **Risk: the release workflow is unproven until a tag.** It cannot be
  dispatched before it reaches `main`. Its steps mirror the CI that is
  already green, and the packaging and installer path were exercised locally
  against a mirror. The first tag (T-12805) is the proof, and it needs owner
  approval.
- **Risk: llama.cpp asset names drift.** The names come from llama.cpp's own
  `release.yml` and `release-publish.yml` at the pinned era. A rename fails
  closed with a named missing asset, and the tag is pinned.
- **Risk: the managed launch is not proven in this sandbox.** The
  pre-existing port-ownership check refuses every engine here, including on
  the pre-sprint tree, while the same lifecycle tests pass on GitHub. The
  downloaded engine and model were proven to answer through Ferric as an
  endpoint. T-12810 investigates.
- **Risk: the 7B default on 12–16 GiB laptops is slow on CPU.** It is the
  model the fleet is measured on, and 1.5B is one flag away. T-12806 brings
  GPU offload.
- **Verdict:** proceed with caveats.
