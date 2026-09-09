# ThumbsUp Roadmap

ThumbsUp is a Windows 11 File Explorer thumbnail handler for `.epub` files.

The project's goal is straightforward:

> Make EPUB thumbnails useful, reliable, and safe in Windows Explorer.

ThumbsUp deliberately keeps that goal narrow. The project is not intended to become a general EPUB reader, converter, renderer, or media-format framework.

This roadmap describes the direction of the project rather than promising specific dates or releases. Priorities may change as new compatibility evidence, security findings, implementation experience, and contributor feedback become available.

## Roadmap Principles

ThumbsUp prioritizes:

1. **Correct EPUB behavior** — Prefer behavior supported by the EPUB specification, established real-world EPUB conventions, or reproducible evidence.
2. **Security** — EPUB files are untrusted input, and thumbnail processing occurs in a sensitive Windows execution environment.
3. **Reliability** — A malformed or unusual EPUB should fail safely rather than destabilize Windows Explorer.
4. **Compatibility** — Support useful real-world EPUBs without turning every observed edge case into a new general rule.
5. **Maintainability** — Keep EPUB parsing, Windows integration, configuration, and installation responsibilities clearly separated.
6. **Evidence over speculation** — New behavior should normally be supported by reproducible examples, tests, specifications, measurements, or other concrete evidence.
7. **Small, reviewable changes** — Prefer focused improvements over broad redesigns.
8. **Understand before expanding** — New capabilities should earn their complexity.

## Current Direction

The current implementation already provides:

- EPUB 3 `cover-image` support.
- EPUB 2 cover metadata support.
- Conventional cover-ID fallbacks.
- EPUB `<guide>` cover fallbacks.
- Optional first-image fallback.
- Path-traversal protection.
- Outer-file, inner-file, and archive-entry limits.
- JPEG, PNG, and GIF image support.
- Cooperative thumbnail-processing deadlines.
- A Windows COM thumbnail provider.
- A configuration GUI.
- Diagnostics and logging.
- Per-user developer registration.
- A signed-MSI installation path.
- Automated Linux and Windows CI.
- Automated release packaging.

These capabilities establish the foundation for the next stage of the project. See the [README](../README.md), [Architecture](ARCHITECTURE.md), [Installation](INSTALL.md), and [Security](SECURITY.md) documentation for the current implementation. 

## Near-Term Priorities

### 1. Real-World EPUB Compatibility

Continue improving cover detection where there is evidence of meaningful compatibility problems.

Priority work includes:

- Reproducible EPUBs that fail to produce an appropriate thumbnail.
- Additional regression fixtures for newly discovered EPUB structures.
- Careful evaluation of EPUB specification requirements and conventions.
- Improvements to existing cover-resolution strategies where evidence supports them.
- Investigation of compatibility reports that can be reproduced and reduced to a minimal example.

Compatibility changes should preserve the existing cover-resolution priority unless there is a clear reason to change it.

A single EPUB, publisher, or authoring tool should not automatically establish a new general rule.

### 2. Security and Defensive Processing

Continue treating security as a core product requirement rather than a separate feature.

Priority work includes:

- Maintaining the existing archive, path, image, and processing-time defenses.
- Testing additional malformed and adversarial EPUB inputs.
- Reviewing changes to parsing and decoding dependencies.
- Keeping unsafe Rust confined to genuine Windows ABI boundaries.
- Reviewing resource-consumption behavior where measurements indicate a potential problem.
- Improving security regression coverage when new attack or failure cases are identified.

Security-sensitive changes should be evaluated against the threat model in [SECURITY.md](SECURITY.md).

### 3. Reliability of the Windows Integration

Improve confidence in the code that runs as part of the Windows thumbnail pipeline.

Priority work includes:

- Regression testing for COM and `IStream` behavior.
- Robust handling of failure paths and unusual Explorer inputs.
- Careful management of thumbnail-processing time and resource use.
- Maintaining the separation between the cross-platform core and Windows-specific code.
- Improving diagnostics when doing so does not add inappropriate complexity to the Explorer-facing path.

The shell extension should remain deliberately small and conservative.

### 4. Test and Fixture Quality

Expand the project's ability to reproduce and prevent real-world problems.

Priority work includes:

- Minimal EPUB fixtures for compatibility bugs.
- Regression tests for security findings.
- Boundary-condition tests for size, entry-count, path, and deadline limits.
- Tests for non-ASCII and unusual archive filenames.
- Windows-specific regression coverage where behavior cannot be meaningfully tested by the core crate alone.
- Maintaining deterministic and reviewable tests.

A good bug report that includes a minimal reproducible EPUB can be more valuable than an unverified implementation proposal.

### 5. Diagnostics and Troubleshooting

Improve the ability to understand why a thumbnail was or was not produced.

Potential work includes:

- Making existing diagnostic information clearer.
- Improving the usefulness of failure information without exposing unnecessary data.
- Making common installation and configuration problems easier to diagnose.
- Ensuring diagnostic behavior remains appropriate for software running in a Windows shell-hosted environment.

Diagnostics should help answer practical questions without turning the shell extension into a general-purpose logging system.

### 6. Documentation and Contributor Experience

Keep the project documentation accurate as the implementation evolves.

Priority work includes:

- Keeping README behavior and installation information current.
- Keeping architecture documentation aligned with implementation.
- Keeping the security model aligned with actual defenses.
- Keeping contributor instructions aligned with CI and release workflows.
- Improving development setup and troubleshooting where recurring problems justify it.
- Documenting important engineering decisions and lessons learned without duplicating information owned by another document.

Documentation should explain the decisions that matter without becoming a second copy of the source code.

## Longer-Term Exploration

The following areas may be worth investigating if sufficient evidence or contributor interest emerges. They are **not commitments**.

### Additional EPUB Compatibility

Investigate additional cover conventions only when supported by reproducible examples and a clear compatibility benefit.

The project should continue to prefer a small, understandable resolution pipeline over an ever-growing collection of heuristics.

### Performance Measurement

Establish useful measurements before pursuing significant performance changes.

Potential areas include:

- Large EPUB processing.
- Image decoding and resizing.
- Archive-entry processing.
- Thumbnail-processing deadlines.
- Explorer-facing responsiveness.

Performance work should demonstrate a measurable problem and a measurable improvement.

### Dependency and Platform Maintenance

Keep Rust, Windows dependencies, image handling, XML handling, ZIP handling, and other important dependencies reasonably current while preserving the project's security and compatibility properties.

Dependency upgrades should be treated as engineering changes when they affect behavior, attack surface, build requirements, or supported environments.

### Broader Windows Compatibility

Consider additional Windows configurations or shell-host scenarios only when there is a demonstrated user need and the maintenance cost is understood.

The project's current focus is Windows 11 File Explorer.

## Deliberately Out of Scope for Now

The following are not current roadmap goals:

- EPUB reading or editing.
- EPUB conversion.
- Full EPUB rendering.
- A general-purpose image thumbnail framework.
- Broad support for arbitrary image codecs without a demonstrated need.
- A cross-platform desktop application.
- A replacement for Windows Explorer or its thumbnail cache.
- Large architectural rewrites without a demonstrated problem.

This list protects the project's central purpose and its security and maintainability constraints.

A future proposal can challenge these boundaries, but it should explain the problem being solved, the evidence for it, the resulting complexity, and the security implications.

## How Work Gets onto the Roadmap

A proposed change is more likely to become roadmap work when it provides:

- A clearly described problem.
- Reproducible evidence.
- A minimal test case or fixture where applicable.
- Relevant EPUB specification or Windows documentation.
- An explanation of compatibility and security implications.
- Tests demonstrating the proposed behavior.
- A reasonable implementation approach.
- An explanation of why the change belongs in ThumbsUp rather than in a separate tool or downstream workflow.

For larger changes, open an issue or discussion before investing heavily in implementation. This allows the problem, constraints, and desired behavior to be established before a particular design becomes difficult to change.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the project's contribution and review expectations.

## Roadmap Status

This roadmap intentionally does not attach target dates to individual items.

Priority should be driven by:

1. Security and reliability issues.
2. Reproducible compatibility problems affecting real EPUBs.
3. Test and diagnostic improvements that make future work safer.
4. Maintenance work required to keep the supported environment healthy.
5. Well-supported improvements that materially improve the user experience without compromising the project's focus.

The roadmap is a guide for making those decisions, not a promise that every listed possibility will be implemented.

## The Standard

ThumbsUp should remain a small project that solves a specific problem well.

The standard for future work is therefore not:

> Can we add this?

It is:

> Does this solve a demonstrated problem, can we verify that it works, and is the added complexity justified by the benefit?

When the answer is yes, the work belongs on the roadmap.

When the answer is unclear, investigate first.