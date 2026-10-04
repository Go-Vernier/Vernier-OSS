# Releasing Vernier

A release is a `v*` tag on `main`. `.github/workflows/release.yml` does the
rest: it builds five binaries, smoke-tests every way to install, creates the
GitHub Release, publishes to npm and updates the Homebrew tap.

## One-time setup

1. **npm.** Create the organisation `go-vernier` on npmjs.com. Create a
   granular access token with read and write on `@go-vernier/*`, and add it
   to this repository as the Actions secret `NPM_TOKEN`.
2. **Homebrew.** Create the public repository `Go-Vernier/homebrew-tap` with
   a README. Create a fine-grained token with Contents read and write on that
   repository only, and add it here as `HOMEBREW_TAP_TOKEN`.

After the first release, switch npm to trusted publishing: on each of the six
packages' settings pages on npmjs.com, add this repository and `release.yml`
as a trusted publisher, then delete `NPM_TOKEN`.

## Cutting a release

1. On `dev`, set `[workspace.package] version` in `Cargo.toml`, and the
   `version` in the root `package.json`, to the new version. Run `cargo
   build` so `Cargo.lock` follows.
2. Add a `## [x.y.z] - YYYY-MM-DD` section to the top of `CHANGELOG.md`, and
   its link at the bottom. The release notes are this section.
3. Open the pull request. CI runs the tests on Linux, macOS and Windows. If
   the pull request touches packaging, the Release workflow dry-runs too;
   otherwise run it by hand from the Actions tab (Release → Run workflow)
   once it is on `main`.
4. Merge to `main`, then tag:

   ```bash
   git switch main && git pull
   git tag v0.1.0 && git push origin v0.1.0
   ```

5. Watch the Release run. The `verify` job installs the published release
   with every one-liner on clean machines; when it is green, the release is
   done.

A version with a suffix (`0.2.0-rc.1`) is a prerelease: a GitHub prerelease,
the `next` tag on npm, and no Homebrew update.

## When something fails

- **check fails:** the tag does not match `Cargo.toml`, or `CHANGELOG.md`
  has no section for the version. Delete the tag (`git push --delete origin
  vX.Y.Z`), fix, and tag again.
- **build or smoke fails:** nothing has been published. Fix on `dev`, merge,
  move the tag.
- **npm or homebrew fails** after the release exists: fix the cause (a
  secret, usually) and re-run the failed job. Packages already published are
  skipped.
- **A broken release reached users:** npm versions cannot be reused. Publish
  a fix as the next patch version; deprecate the broken one with `npm
  deprecate @go-vernier/cli@X.Y.Z "use X.Y.Z+1"`.

## The README demo

`docs/demo.tape` records `vernier tui` on robot-shop with
[VHS](https://github.com/charmbracelet/vhs). Run the Demo workflow from the
Actions tab, download the `demo` artifact, and commit it as `docs/demo.gif`.
