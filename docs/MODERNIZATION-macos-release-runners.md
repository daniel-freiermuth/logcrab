# macOS Release Runners

*Decision record for the macOS legs of `.github/workflows/release.yml`. Evidence gathered 2026-09-29.*

## Current State

| Matrix id       | Runner label     | Image / default Xcode        | SDK in shipped binary | Minimum macOS (`minos`) |
|-----------------|------------------|------------------------------|-----------------------|-------------------------|
| `macos-aarch64` | `macos-15`       | macOS 15 arm64 / Xcode 16.4  | 15.5 (expected)       | 11.0 (expected)         |
| `macos-x86_64`  | `macos-15-intel` | macOS 15 x64 / Xcode 16.4    | 15.5                  | 10.12                   |

Until 2026-09-29 the aarch64 leg ran on `macos-14`. The v1.3.0 aarch64 binary reports SDK 14.5 / `minos` 11.0. The v1.3.0 x86_64 binary reports SDK 15.5 / `minos` 10.12 (`llvm-objdump --macho --private-headers`). The "expected" values for the new aarch64 leg still need to be confirmed from the first `macos-15` build. See "Verification" below.

The release workflow never runs the built binary on any leg. CI (`ci.yml`) runs `just check` on `ubuntu-latest` only. So the macOS runners provide **build** coverage, not **test** coverage.

## Evidence

- **`macos-14` retirement:** [actions/runner-images#13518](https://github.com/actions/runner-images/issues/13518) (open). Deprecation began 2026-07-06, retirement is 2026-11-02, and brownouts run 2026-10-05 through 2026-10-31. Jobs using the label "will be terminated with an error". Ubuntu 22.04/24.04 are not affected.
- **Intel label horizon:**
  - [actions/runner-images#13045](https://github.com/actions/runner-images/issues/13045): `macos-15-intel` "will be available from now until August 2027. This will be the last available x86_64 image from Actions".
  - [GitHub Changelog 2025-09-19](https://github.blog/changelog/2025-09-19-github-actions-macos-13-runner-image-is-closing-down/) says the same.
- **Conflicting later signal:** [GitHub Changelog 2026-02-26](https://github.blog/changelog/2026-02-26-macos-26-is-now-generally-available-for-github-hosted-runners/) made `macos-26-intel` GA as a "Standard macOS runner for x64". The GitHub-hosted runners docs list it next to `macos-15-intel`. No retirement date for `macos-26-intel` has been published, and no revision of the "x86_64 ends August 2027" statement was found.
- **Apple:** macOS 26 Tahoe is the last macOS release for Intel Macs, and macOS 27 is Apple silicon only ([MacRumors, 2026-04-18](https://www.macrumors.com/2026/04/18/macos-27-compatibility-change/)).
- **Usage:** macOS tarballs first shipped in v1.2.0 (2026-09-01). GitHub download counts as of 2026-09-29:
  - v1.2.0: 1 download each for x86_64 and aarch64
  - v1.3.0: 0 downloads each

## Done: Apple Silicon Leg → `macos-15`

The label moved from `macos-14` to `macos-15`, not `macos-26` or `macos-latest`:

- `macos-15` arm64 defaults to Xcode 16.4 / macOS 15.5 SDK. That is the same SDK the x86_64 artifact already ships with, so both macOS artifacts are linked against one SDK.
- `macos-26` defaults to Xcode 26.6 (macOS 26 SDK). Linking against a new major SDK opts AppKit into SDK-gated behavior changes, for example the macOS 26 window chrome. That would be a separate, user-visible change. *[Inference: not tested. egui draws its own content, so only native window chrome would be affected.]*
- `macos-latest` floats. The `ubuntu-latest` → 26.04 migration (Oct 19 – Nov 19, 2026) shows what that does to release builds.
- Tooling the workflow relies on is identical between the macos-14 arm64 and macos-15 arm64 images: jq 1.8.2, bsdtar, git, rustup 1.29.

## Open Decision: Intel (x86_64) Leg

Nothing breaks before August 2027. This decision is **not** urgent. It is recorded here so it is made on purpose rather than forced by a failed release. If the leg fails, the whole GitHub release is blocked, because `publish` has `needs: build`.

| Option | Work | Hard deadline | Risk / cost |
|--------|------|---------------|-------------|
| **A. Keep `macos-15-intel`** | None now | August 2027 (#13045) | Must revisit before then. Zero change today. |
| **B. Move to `macos-26-intel`** | 1 line | None announced | Lifespan unknown and contradicts GitHub's stated x86_64 end. Moves Intel users to the macOS 26 SDK while arm64 stays on 15.5. |
| **C. Cross-compile on the arm64 runner** | ~10 lines | None (no Intel host needed) | The x86_64 binary is no longer built on Intel hardware. Nothing is lost today: the release workflow never executes it. |
| **D. Drop the Intel artifact** | Remove matrix entry | None | Breaking for Intel Mac users. Low observed demand (see Usage). |

### Recommendation: C (cross-compile), in its own PR, any time before mid-2027

- It removes the dependency on GitHub's Intel fleet entirely, instead of trading one retiring label for one with an unknown date.
- Both macOS artifacts are built by one image and one SDK.
- The build is cross-compile-safe:
  - `build.rs` only runs host tools: `protoc` via `tonic-prost-build`, and `git`.
  - No C code is compiled for macOS targets. `cargo tree --target all -i cc` shows `cc` only under android-activity, iana-time-zone-haiku and wayland-backend.
  - `objc-sys` / `core-foundation-sys` only emit link directives.
- Option D stays available later with no wasted work. Revisit it when macOS 26 security support ends (expected fall 2028).

### Migration Steps (Option C)

1. On the `macos-x86_64` matrix entry, set `os: macos-15` and add `target: x86_64-apple-darwin`.
2. Install the target:
   ```yaml
   - uses: dtolnay/rust-toolchain@stable
     with:
       targets: ${{ matrix.target }}
   ```
   An empty `targets` for the other legs is fine.
3. In the build step, pass `--target "$TARGET"` when `matrix.target` is set.
4. In the assemble step, tar from `target/${TARGET}/release` when set, otherwise from `target/release`.
5. Leave artifact names unchanged (`logcrab_x86_64_macos.tar.gz`). Nothing downstream changes.

**Effort:** under an hour of editing, plus one `workflow_dispatch` run (see below).

**Rollback:** revert the PR. `macos-15-intel` remains available until August 2027.

**Unverified:** Rosetta 2 is not listed in the macos-15 arm64 image README. Executing the x86_64 binary on that runner is therefore not guaranteed. This is not needed today because no leg executes its binary.

## Minimum macOS Version (`MACOSX_DEPLOYMENT_TARGET`)

The runner OS does **not** determine the shipped minimum macOS version. The shipped binaries carry exactly rustc's per-target defaults: `minos` 11.0 (aarch64, built on macOS 14) and 10.12 (x86_64, built on macOS 15). rustc 1.98 `--print deployment-target` shows:

| Target | Unset | `=10.12` | `=11.0` | `=15.0` |
|--------|-------|----------|---------|---------|
| `aarch64-apple-darwin` | 11.0 | 11.0 (clamped) | 11.0 | 15.0 |
| `x86_64-apple-darwin`  | 10.12 | 10.12 | 11.0 | 15.0 |

rustc clamps any lower value up to its per-target minimum, and no C code is compiled for macOS. So pinning `MACOSX_DEPLOYMENT_TARGET` to the current values would be a no-op, and it was not added.

Setting it only matters for **raising** the floor, which is a product decision. Neither 10.12 nor 11.0 has ever been tested; they are toolchain defaults, not support claims.

## Verification

`workflow_dispatch` of `release.yml` with `ref` set to the PR branch builds all four legs. **Caution:** for `workflow_dispatch`, the `publish` job always treats the run as a release. It is only stopped by the tag check:

- If `Cargo.toml` on that ref has a version whose tag already exists at a *different* commit (currently `1.3.0` → tag at `a28cc4d`), `publish` fails on purpose and nothing is published.
- If the version is untagged, a real GitHub release is created.

After the run, download the `release-macos-aarch64` artifact and check `vtool -show-build logcrab` (or `otool -l`) for `minos 11.0` / `sdk 15.5`.

## Upcoming Deadlines

| Date | Event |
|------|-------|
| 2026-10-05 → 2026-10-31 | `macos-14` brownouts (no longer used) |
| 2026-11-02 | `macos-14` retired (no longer used) |
| August 2027 | `macos-15-intel` retired: Intel leg must have moved (options B/C/D) |
| Not announced | `macos-15` arm64 retirement. Per GitHub's "latest two stable versions" policy (#13518), expect a deprecation notice after a macOS 27 image goes GA. *[Inference]* |
