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
4. **Check it.** Download an installer or two and read the job summaries. A `::warning::` means a
   signing secret was missing and that artifact is unsigned.
5. **Publish** the draft in the GitHub UI. Publishing creates the tag. A version with a suffix
   (`-alpha.1`, `-rc.1`) is marked as a pre-release.

Pushing to `release` again before publishing rebuilds the same draft. Once published, bump the
version before releasing again.

**Test runs:** *Actions > Release > Run workflow* with an optional `version` override. On any
branch other than `release` it is a dry run: the signing jobs are refused by the `release`
environment and no draft is created.

## What gets built

| Platform | Artifacts |
|---|---|
| macOS (universal) | `.dmg`, CLI `.zip` |
| Windows x64, x86, ARM64 | `.msi`, portable `.zip` |
| Linux x86_64, aarch64 | `.AppImage`, `.deb`, `.rpm`, `.tar.gz`, `.flatpak`, Arch package |
| Linux riscv64 | `.tar.gz` |
| FreeBSD x86_64 | `.tar.gz` |

## One-time GitHub setup

- Create the `release` branch (from `main`).
- Create a GitHub environment named `release`, restricted to the `release` branch.
- Optional signing secrets (without them artifacts are unsigned, which is fine for an alpha):
  `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD`, `APPLE_ID`,
  `APPLE_PASSWORD`, `APPLE_TEAM_ID`, `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD`,
  `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`,
  `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE`.

## Packaging layout

`packaging/` holds the per-platform scripts the shared workflow calls (`macos/`, `windows/`,
`linux/`, `freebsd/`), `env.sh` and `icons.sh`. The app icon sources live in `assets/app-icon/`;
regenerate the derived formats with `packaging/icons.sh`.
