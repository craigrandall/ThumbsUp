# Code Signing Status

**Current state: ThumbsUp releases are not code-signed.** This page is the
single source of truth for that fact — if any other document in this repo
says otherwise, that document is wrong and should be corrected to link
here instead of restating its own version.

## Why

A Windows Authenticode code-signing certificate costs real money on an
ongoing basis (roughly $75–400/year for a standard OV certificate from a
commercial CA, or ~$10/month for Microsoft's own Azure Artifact Signing
service) unless issued free to a qualifying open-source project. ThumbsUp
has applied to the two free-for-OSS programs we're aware of and been
turned down by both, on the stated grounds of insufficient project
visibility/maturity rather than any concern about the project itself:

- **SignPath.org** (OSS free-signing program) — declined, cited lack of
  current project visibility.
- **OSSign** (<https://ossign.org>) — does not yet meet published
  eligibility criteria.

We are not paying out of pocket for a certificate for a free project.
Both programs' bars are about project traction (stars, releases, real
usage), not code quality, so re-applying later as the project grows is
the intended path — see "What would change this" below.

## What this means for you

Because the DLL and installer are unsigned:

- Windows SmartScreen will likely show a warning on first run of
  `ThumbsUp.msi` ("Windows protected your PC"). Clicking **More info →
  Run anyway** proceeds with installation.
- Some enterprise application-allowlisting policies may block unsigned
  installers entirely. If your organization requires signed software,
  this project does not currently meet that bar — we're not going to
  pretend otherwise.
- Right-click → Properties → Digital Signatures will show **no signature
  tab at all** on the `.msi`, `.dll`, or `.exe`. This is expected; it does
  not indicate a corrupted download.

## How to verify integrity without a signature

A signature proves *identity* (who published this) as much as it proves
*integrity* (the bytes weren't tampered with in transit). Absent a
signature, you can still verify integrity:

1. Every GitHub Release publishes a `SHA256SUMS.txt` alongside the
   binaries. Compare the hash of what you downloaded against that file.
2. Every release is built by the [`release.yml`](../.github/workflows/release.yml)
   GitHub Actions workflow directly from the tagged commit — you can read
   the workflow yourself and confirm it does nothing except `cargo build`
   and `light.exe`, or rebuild from source and diff.
3. GitHub Actions attaches a build-provenance attestation (via
   [`actions/attest-build-provenance`][attest]) to each release artifact.
   This cryptographically ties a given binary to the exact workflow run,
   commit, and source repository that produced it — verifiable with
   `gh attestation verify`. This is **not** the same thing as an
   Authenticode signature (Windows/SmartScreen do not check it, and it
   proves provenance, not publisher identity) but it is a real,
   independently-verifiable supply-chain guarantee, and it costs nothing.

## What would change this

- **Re-apply to SignPath/OSSign** once the project has more of the
  traction signals those programs look for: sustained releases, a real
  user base, external contributors, issue/PR activity. This is the
  intended long-term path and costs nothing.
- **Pay for signing** if the calculus changes — the cheapest current
  option is Microsoft's own **Azure Artifact Signing** (~$9.99/month,
  identity-verified, eligibility currently limited to individuals in
  select countries — check current terms before assuming eligibility).
  Not planned while this remains a self-funded free project.
- **Distribute via the Microsoft Store** instead of (or alongside) a
  standalone MSI — the Store signs submitted packages as part of
  certification, at a one-time ~$19 individual developer account cost
  rather than a recurring fee. This would require packaging the shell
  extension as MSIX with sparse-package COM registration, which is a
  nontrivial repackaging effort not currently scheduled.

[attest]: https://github.com/actions/attest-build-provenance
