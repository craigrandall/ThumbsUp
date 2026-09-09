# Contributing to ThumbsUp

Thank you for your interest in contributing to ThumbsUp!

ThumbsUp is an open-source Windows 11 File Explorer thumbnail handler for `.epub` files. It is implemented in Rust and includes a cross-platform EPUB parsing core, a Windows COM shell extension, and a configuration GUI.

Contributions are welcome, particularly bug reports, EPUB compatibility improvements, security improvements, tests, documentation, and thoughtful improvements to the Windows integration.

Please read this document before starting work so that contributions fit the project's architecture, quality standards, and development workflow.

## 1. Getting Started

### Clone the repository

```powershell
git clone https://github.com/craigrandall/ThumbsUp.git
cd ThumbsUp
```

### Development prerequisites

For development of the parsing core:

- Rust stable toolchain
- Rust 1.74 or later
- Cargo

For Windows shell-extension and GUI development:

- Windows 11
- Rust stable with the MSVC toolchain
- Visual Studio 2022 Build Tools with the C++ workload
- Windows 11 SDK 22621 or later

For building the installer:

- WiX Toolset 3.11 or 3.14

WiX 4 is not currently supported because the installer authoring is written for WiX 3.

See [`docs/INSTALL.md`](docs/INSTALL.md) for the detailed development-environment setup.

## 2. Project Structure

ThumbsUp is organized as a Cargo workspace:

- **`crates/thumbsup-core/`** – Cross-platform EPUB parsing, cover resolution, path handling, image processing, and thumbnail generation.
  - `container.rs` – EPUB container parsing.
  - `opf.rs` – OPF parsing and cover-resolution logic.
  - `path.rs` – EPUB path resolution and traversal protection.
  - `image_ops.rs` – Image decoding, resizing, and BGRA8 conversion.
  - `cover.rs` – Top-level thumbnail extraction pipeline.
  - `error.rs` – Core error types.
  - `tests/` – Integration tests and EPUB fixture construction.

- **`crates/thumbsup-shell/`** – Windows-only COM shell-extension DLL.
  - Implements `IThumbnailProvider` and the associated COM class factory.
  - Handles Windows `IStream` input and conversion to Windows bitmap objects.
  - Contains registration, configuration, logging, and COM-related integration.

- **`crates/thumbsup-config/`** – Windows configuration GUI built with `eframe`/`egui`.
  - Provides user-facing configuration.
  - Reads and writes the ThumbsUp registry configuration.
  - Provides cache-clearing and diagnostic-log functionality.

- **`installer/`** – WiX installer authoring and installer resources.

- **`scripts/`** – Developer and build scripts, including installer creation and DLL registration.

- **`docs/`** – Architecture, installation, and security documentation.

- **`.github/workflows/`** – CI and release workflows.

Keep module responsibilities clear. In particular, prefer keeping EPUB parsing and policy in `thumbsup-core` rather than coupling the core implementation to Windows APIs.

## 3. Development Workflow

1. **Start with an issue or discussion.**

   Before beginning substantial work, open or identify a GitHub Issue describing the bug, feature, compatibility problem, security concern, or other proposed change.

   For smaller changes, use your judgment; not every documentation or trivial maintenance change requires an issue first.

2. **Understand the relevant documentation and prior art.**

   Before changing EPUB behavior, understand the applicable EPUB specification and examine existing implementation and test coverage.

   ThumbsUp intentionally supports both specification-defined behavior and selected real-world EPUB conventions. Do not change cover-resolution behavior based solely on a single EPUB encountered in testing.

3. **Create a branch.**

   ```powershell
   git checkout -b feature/<short-name>
   ```

4. **Implement the change.**

   Favor:

   - Clear separation of concerns.
   - Strong typing.
   - Explicit error handling.
   - Small, testable functions.
   - Defensive handling of untrusted EPUB content.
   - Behavior supported by specifications, evidence, or clearly documented project policy.

5. **Add or update tests.**

   A behavioral change should normally include corresponding test coverage.

   In particular, changes to EPUB parsing or cover resolution should include representative fixtures and edge cases rather than relying solely on manual testing.

6. **Run the local verification sequence.**

   At minimum:

   ```powershell
   cargo fmt --all
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features
   cargo build --release
   ```

   Windows-specific changes should also be built and tested using the Windows MSVC target.

7. **Review the resulting changes.**

   Before submitting a pull request:

   ```powershell
   git diff
   git status
   ```

   Confirm that generated files, local configuration, diagnostic output, binaries, and unrelated changes have not been included.

8. **Submit a pull request.**

   The pull request should explain:

   - What changed.
   - Why the change is needed.
   - How the implementation works at an appropriate level.
   - What tests were added or changed.
   - What verification was performed.
   - Any compatibility, security, performance, or behavioral implications.

## 4. Coding Standards

Use idiomatic Rust and preserve the architectural boundaries already established in the project.

### General principles

- Prefer clear, small, composable functions.
- Use `Result` and `Option` deliberately.
- Propagate errors with appropriate context.
- Avoid unnecessary allocation and copying, particularly in code executed by the Explorer shell extension.
- Prefer explicit behavior over cleverness.
- Keep security-sensitive behavior easy to review.
- Do not introduce `unsafe` into `thumbsup-core`.
- Keep Windows-specific concerns out of the cross-platform core.

### `thumbsup-core`

The core is responsible for EPUB and image processing and should remain independent of Windows shell APIs.

The core parses untrusted ZIP, XML, XHTML, and image input. Changes must preserve its defensive behavior, including:

- Path-traversal protection.
- Outer EPUB size limits.
- Inner archive-entry size limits.
- Archive entry-count limits.
- Image-format restrictions.
- Thumbnail deadline enforcement.
- Graceful handling of malformed EPUB content.

Do not weaken these protections merely to accommodate a problematic EPUB without first understanding the security implications.

### `thumbsup-shell`

The shell extension runs inside Windows Explorer and therefore deserves particular care.

Changes should:

- Minimize work performed on the Explorer-facing path.
- Avoid unnecessary threading or global state.
- Preserve the COM contract.
- Keep `unsafe` code confined to genuine Windows ABI/interoperability boundaries.
- Document non-obvious safety invariants.

### `thumbsup-config`

Keep GUI concerns separate from configuration and registry behavior where practical.

Changes to configuration behavior should consider:

- Existing registry values.
- Backward compatibility.
- User-visible defaults.
- Diagnostic behavior.
- Documentation.

## 5. EPUB Behavior and Compatibility

ThumbsUp deliberately supports more than the minimum happy-path interpretation of the EPUB specification.

The current cover-resolution pipeline includes:

1. EPUB 3 `properties="cover-image"`.
2. EPUB 2 `<meta name="cover" content="...">`.
3. Conventional cover IDs such as `cover`, `cover-image`, and `ci`.
4. `<guide>` `thumbimagestandard`.
5. `<guide>` `cover` references pointing directly to an image.
6. `<guide>` `cover` references pointing to XHTML containing an image.
7. An optional first-image fallback.

When modifying this behavior:

- Establish what the EPUB specification says.
- Examine existing project behavior and tests.
- Consider real-world EPUB compatibility.
- Preserve the intended priority order unless there is a documented reason to change it.
- Add regression coverage for newly discovered formats or edge cases.
- Document significant changes in behavior.

A single publisher's EPUB should not automatically establish a new general rule.

## 6. Testing

Testing is particularly important because ThumbsUp processes files supplied to Windows Explorer, including malformed or potentially hostile EPUB content.

### Core tests

Run:

```powershell
cargo test -p thumbsup-core
```

The core test suite covers EPUB 2 and EPUB 3 behavior, malformed inputs, path traversal, image-format handling, guide fallbacks, non-ASCII filenames, deadline enforcement, and end-to-end extraction.

When changing parsing or cover resolution, prefer adding a focused regression test or fixture rather than relying only on manual inspection.

### Workspace tests

Run:

```powershell
cargo test --workspace --all-features
```

### Windows tests

Windows-specific crates should be tested on Windows:

```powershell
cargo test -p thumbsup-shell
cargo test -p thumbsup-config
```

For changes affecting the Windows shell extension or configuration GUI, also perform the relevant manual verification described in `docs/INSTALL.md`.

### Test principles

- Test behavior, not implementation details.
- Test malformed input as well as valid input.
- Include boundary conditions.
- Preserve deterministic tests.
- Prefer minimal fixtures that demonstrate the behavior being tested.
- Add regression coverage for bugs before changing the implementation.

## 7. Security

Security is a first-class concern in ThumbsUp.

The shell extension parses untrusted EPUB content while executing inside Explorer. Contributions must therefore consider security implications, particularly around:

- ZIP/archive processing.
- XML and XHTML parsing.
- Image decoding.
- Path resolution.
- Resource exhaustion.
- Memory usage.
- Time spent processing a thumbnail.
- Windows COM and ABI boundaries.

Read [`docs/SECURITY.md`](docs/SECURITY.md) before making security-sensitive changes.

Potential security vulnerabilities should **not** be disclosed publicly in an issue before they have been evaluated. Follow the repository's security policy for responsible disclosure.

## 8. Documentation

ThumbsUp uses several forms of documentation, each with a different purpose.

### `README.md`

Update the README when changing:

- User-visible behavior.
- Supported EPUB behavior.
- Installation requirements.
- Configuration.
- Build requirements.
- Known limitations.
- Major project structure or workflow.

### `docs/`

Use the appropriate document for deeper technical material:

- `docs/ARCHITECTURE.md` – System structure and design.
- `docs/INSTALL.md` – Development and installation setup.
- `docs/SECURITY.md` – Security model and threat considerations.

### Lessons learned and engineering notes

The project also maintains engineering/lessons-learned documentation. These documents capture reasoning and experience that may be useful to future contributors.

Do not duplicate implementation details unnecessarily. Prefer updating an existing document when it already owns the subject.

### Documentation consistency

If documents disagree:

- Source code and its comments are authoritative for actual behavior.
- Security documentation is authoritative for the documented security model.
- Architecture/design documents explain system structure and rationale.
- README and CONTRIBUTING.md provide contributor/user-facing summaries.

When behavior changes, update the appropriate documentation rather than allowing documentation drift.

## 9. AI-Assisted Development and Verification

AI-assisted development is permitted and may be useful, particularly for learning Rust, exploring unfamiliar APIs, generating test ideas, or reviewing alternatives.

However:

**AI-generated output is not considered verified merely because it appears plausible or because an AI system expressed confidence in it.**

Contributors remain responsible for understanding and verifying the changes they submit.

In particular:

- Do not blindly accept AI-generated code.
- Verify recommendations against the Rust documentation, relevant specifications, project architecture, and existing code.
- Pay particular attention to security-sensitive code and Windows ABI boundaries.
- Add tests for behavior introduced or changed with AI assistance.
- Run the project's verification commands locally.
- Clearly disclose relevant AI assistance in the pull request when it materially contributed to the change.

The principle is simple:

> **Know what you leverage.**

Understanding the problem domain makes AI assistance more useful and makes it possible to recognize incorrect assumptions, challenge recommendations, and verify proposed solutions.

## 10. Continuous Integration

ThumbsUp uses GitHub Actions for automated verification.

### Core verification

The CI workflow runs on pushes to `main` and pull requests targeting `main`.

The core verification runs on Linux and includes:

```powershell
cargo fmt --all -- --check
cargo clippy -p thumbsup-core -- -D warnings
cargo check -p thumbsup-core
cargo test -p thumbsup-core --all-features
```

### Windows verification

The Windows CI job uses the stable Rust MSVC toolchain and verifies:

- Formatting.
- `thumbsup-shell` Clippy checks.
- `thumbsup-config` Clippy checks.
- Windows-target compilation.
- Shell-extension tests.
- Configuration-GUI tests.
- Release builds of the shell extension and configuration GUI.

A pull request should be expected to pass both the core and Windows verification jobs.

## 11. Releases

Releases are automated through `.github/workflows/release.yml`.

A release is triggered by pushing a tag matching:

```text
v*
```

For example:

```text
v0.1.0
v0.2.0
v1.0.0
```

Before creating a release:

1. Update the project version as appropriate.
2. Update `CHANGELOG.md`.
3. Ensure the release is based on the intended `main` commit.
4. Run the local verification sequence.
5. Create and push the version tag.

The release workflow performs its own verification before producing release artifacts.

Do not create a release tag from an unverified or unpublished development commit.

## 12. Pull Request Expectations

A good pull request should be:

- Focused on one logical change.
- Small enough to review effectively where practical.
- Supported by tests.
- Consistent with the existing architecture.
- Explicit about behavior changes.
- Honest about verification performed.

Please do not claim that a command, test, build, or manual verification was performed unless it actually was.

If a verification step could not be performed, say so explicitly.

A useful pull request description should allow a reviewer to answer:

1. What problem does this solve?
2. Why is this approach appropriate?
3. What evidence supports the behavior?
4. What tests demonstrate it?
5. What risks or limitations remain?

## 13. What Contributions Are Especially Useful?

ThumbsUp is an intentionally focused project. Useful contributions include:

- Reproducible EPUB compatibility problems.
- Minimal EPUB fixtures demonstrating a parsing or cover-resolution issue.
- Additional regression tests.
- Security improvements.
- Performance improvements supported by measurement.
- Improvements to Windows integration.
- Documentation improvements.
- Improvements to the development and testing workflow.
- Careful review of EPUB specification interpretation.
- Bug fixes accompanied by evidence and tests.

If you have an idea for a larger feature or architectural change, please open a discussion or issue before investing heavily in implementation. This gives us an opportunity to establish the problem, constraints, and desired behavior before committing to a particular design.

## 14. Final Principle

ThumbsUp is a project about more than producing EPUB thumbnails. It is also an exercise in understanding a problem domain, studying specifications and prior art, making evidence-based engineering decisions, and building software that can safely operate in a sensitive execution environment.

Contributors are encouraged to bring that same mindset:

**Understand what you are building, understand what you are leveraging, and verify what you change.**