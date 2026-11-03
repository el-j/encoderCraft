# Deferred release / packaging workflows

These workflows were adopted from the ArtCraft app template (vectorCraft / filmCraft) but are **not
active** in `.github/workflows/`, because every one of them needs a `packaging/` tree that
encoderCraft does not have yet. In CI they failed immediately (`Cannot find path
'packaging/windows/install-policy.json'`, `Get-ChildItem` on a missing `packaging/`).

| File | Needs |
|---|---|
| `packaging-lint.yml` | `packaging/` (+ `packaging/check-app-specific.sh`, `packaging/windows/install-policy.json`, `test-install-policy.ps1`) |
| `windows-arm64.yml` | `packaging/windows/**` (WiX installer for `EncoderCraft`) |
| `freebsd.yml` | `packaging/freebsd/package.sh`, `packaging/env.sh` |
| `release.yml` | all of the above plus macOS / Linux / Windows packaging, signing secrets, `docs/releasing.md` |

## Re-enabling

1. Create `packaging/` for encoderCraft (start from `storytold/filmcraft/packaging`, rename the app
   id/desktop file/metainfo, and use only original or CC0/MIT icons listed in `ASSETS.md`).
2. Add `assets/app-icon/**`.
3. `git mv docs/ci/deferred-workflows/<file>.yml .github/workflows/`.
4. `release.yml` also requires the craft-fonts pin only if the app embeds fonts (encoderCraft does
   not: drop `craft-fonts-ref` / `web-craft-fonts`).

The `git-sources` job in `release.yml` (all git dependencies must come from `github.com/storytold/`)
already passes for this repository.
