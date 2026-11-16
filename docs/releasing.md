# Releasing EncoderCraft

Every push to the `release` branch runs `.github/workflows/release.yml`, which calls the shared
`storytold/craft-common-actions` release workflow (the same one VectorCraft, SoundCraft, FilmCraft,
PhotoCraft and EffectCraft use). It builds installers for macOS, Windows, Linux and FreeBSD and
creates or updates a **draft** GitHub Release named `EncoderCraft v<version>`. Nobody sees a draft
until a maintainer publishes it. There is no web build.

User-facing names say **EncoderCraft**. Files, binaries and ids stay lowercase
(`encodercraft-<version>-<platform>-<arch>.<ext>`, `ai.storyteller.encodercraft`).

## Cutting a release

1. **Bump the version** on `main`. It lives in exactly one place, `[workspace.package] version`
   in the root `Cargo.toml`:

   ```sh
   cargo xtask version                      # prints the current version
   cargo xtask version set 0.1.0-alpha.1    # updates Cargo.toml and Cargo.lock
   ```

   Commit it as `Release: EncoderCraft v0.1.0-alpha.1`.
2. **Merge `main` into `release`** (or fast-forward it) and push. The workflow starts by itself.
3. **Wait for the draft.** The Releases page gets a draft with every artifact and
   `SHA256SUMS.txt`. Notes are generated from the merged pull requests.
4. **Check it.** The `release-smoke` workflow runs on clean runners. It unpacks the Linux tarball,
   Windows portable zip and macOS CLI zip, then installs the Linux deb/rpm, runs the AppImage,
   mounts and installs the macOS DMG, and silently installs the Windows MSI. It checks GUI and CLI
   versions, real PNG and MP4 transcodes, licenses, and platform resources. Run it by hand with
   *Actions > release-smoke > Run workflow*. The workflow reads the draft by its tag, so if it
   cannot find the draft, download the assets and run the matching script under
   `packaging/smoke/`.
5. **Complete manual QA.** Run the human-only checks in
   [manual-release-qa.md](./manual-release-qa.md) against the exact candidate and retain the
   results with the release record. Synthetic CI fixtures do not replace testing phone footage or
   screen recordings.
6. **Publish** the draft in the GitHub UI. Publishing creates the tag. A version with a suffix
   (`-alpha.1`, `-rc.1`) is marked as a pre-release.

Pushing to `release` again before publishing rebuilds the same draft. Once published, bump the
version before releasing again.

**Test runs:** *Actions > Release > Run workflow* with an optional `version` override. Jobs
using the `release` environment run only from branches allowed by its deployment rules.
An allowed non-`release` branch builds unsigned and, if all required jobs succeed, creates
or updates a draft release; it is not a dry run. The workflow refuses to update an already
published release. To test without publishing, run the matching smoke scripts locally or
dispatch `release-smoke` with the Release run ID and version.

## What gets built

| Platform | Artifacts |
|---|---|
| macOS (universal) | `.dmg`, CLI `.zip` |
| Windows x64, x86, ARM64 | `.msi`, portable `.zip` |
| Linux x86_64, aarch64 | `.AppImage`, `.deb`, `.rpm`, `.tar.gz`, `.flatpak`, Arch package |
| Linux riscv64 | `.tar.gz` |
| FreeBSD x86_64 | `.tar.gz` |

FreeBSD aarch64 is temporarily excluded: the `iceoryx2` 0.10.0 dependency fails to
compile for that target because its FreeBSD implementation assumes a signed `c_char`.
Restore the target once an upstream fix is available and the release build passes.

## One-time GitHub setup

- Create the `release` branch (from `main`).
- Create a GitHub environment named `release`, restricted to the `release` branch.
- Signing secrets in the `release` environment. **They are required:** on the `release` branch
  the shared workflow fails the macOS and Windows jobs when they are empty, and no draft is
  created. macOS needs `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD`,
  `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`. Windows needs either `WINDOWS_CERTIFICATE` +
  `WINDOWS_CERTIFICATE_PASSWORD` or the Azure Trusted Signing set (`AZURE_TENANT_ID`,
  `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`,
  `AZURE_CERT_PROFILE`). Pushes to other branches never sign and never create a draft.

## Packaging layout

`packaging/` holds the per-platform scripts the shared workflow calls (`macos/`, `windows/`,
`linux/`, `freebsd/`), `env.sh` and `icons.sh`. The app icon sources live in `assets/app-icon/`;
regenerate the derived formats with `packaging/icons.sh`.

## Unsigned alpha releases

Signing secrets are required on the `release` branch. To ship an unsigned pre-release without
forking the shared workflow, create a dedicated branch at the intended `main` commit and
temporarily allow that exact branch in the `release` environment's deployment rules. Dispatch
*Actions > Release > Run workflow* from that branch. The workflow builds unsigned artifacts and
creates a draft when all required jobs succeed. Remove the temporary environment rule after the
run, smoke-test the draft, and explicitly disclose the absence of signatures before publishing.
Unsigned macOS and Windows downloads may trigger operating-system security warnings.

## Release notes: known issues

Every pre-release body ends with this section; copy it forward and edit it:

```md
## Known issues
- <what does not work yet, with the tracking issue number>

Newer builds fix problems listed in earlier notes; see the latest release.
```
