# Oceans CLI binary release research

Research date: 5 October 2026. Status: proposal for review. No release workflow changes or binary publication were performed.

For the current requirement, use a native GitHub Actions build matrix with repository tasks run through `mise`. Keep Cocogitto for version commits and tags, git-cliff for changelogs and notes, and the GitHub CLI for release publication. Add cargo-dist if the distribution scope expands to generated installers or package managers.

The main design change is release ownership. The local command should prepare and push the release source. CI should publish the GitHub release after the required artifacts pass validation. This gives the CLI, container images, and Helm chart one product version and one completion point.

## Research scope and evidence

Exa returned 71 search results in four rounds, with 64 distinct URLs before URL normalization. The searches covered binary packaging, version and changelog integration, platform support, and publication failure modes. Later rounds tested the most suitable options against this repository. Conclusions use project documentation, maintainer source code, and current local files. Search results were discovery aids; they do not prove that Oceans builds on a target platform.

PR [407](https://github.com/ahstn/oceans-llm/pull/407) merged on 4 October 2026. A live GitHub CLI read confirmed that the latest release, [v0.37.0](https://github.com/ahstn/oceans-llm/releases/tag/v0.37.0), has no uploaded release assets. GitHub's automatic source archives are separate from this asset list.

## Current repository contract

The CLI package is `oceans-cli`; its executable is `oceans`. Its version is `0.37.0`, matching the current product release. It depends on the shared `gateway-skills` crate. These facts come from [the CLI manifest](../../../crates/oceans-cli/Cargo.toml).

The [release task](../../../mise.toml) runs `cog bump --auto`, pushes `main` and the tag atomically, then creates a published GitHub release with git-cliff notes. The [tag workflow](../../../.github/workflows/release.yml) publishes container images and then the Helm chart. It does not build CLI archives.

The accepted [release ADR](../../adr/2026-03-06-release-versioning-and-ghcr-publishing.md) selects one product version and deliberate local release preparation. It rejects release PRs and automatic public releases after every merge. The binary distribution proposal preserves those choices but moves final GitHub publication to CI. If adopted, record that change in a new ADR and add a supersession note to the existing ADR.

## Options narrowed by repository fit

The ranking below is an engineering assessment. It gives priority to the requested binary downloads, existing release ownership, `mise` task execution, and a small migration.

| Approach | Verified capability | Assessment for Oceans |
| --- | --- | --- |
| Native build matrix, `mise` tasks, and `gh` | Cargo builds selected packages; the GitHub CLI creates releases and uploads assets. | Recommended now. The repository keeps direct control of build checks and publication order. Maintainers must own archive and checksum tasks. |
| `taiki-e/upload-rust-binary-action` | Builds Linux, macOS, and Windows binaries; makes archives and checksums; supports Cargo, cross, and cargo-zigbuild. | Closest packaged alternative. It reduces packaging code but combines build and upload operations that need separation for a complete product release. |
| cargo-dist, now called dist | Builds archives, installers, manifests, and generated release CI; supports GitHub hosting and package publishing. | Best broader distribution tool. It requires a larger workflow integration than binary archives alone. |
| cross or cargo-zigbuild | Provides cross-compilation; cross also supports cross-testing. | Optional build helpers. Neither replaces release orchestration, changelogs, or asset publication. |
| release-plz, cargo-release, or release-please | Manages release preparation, package versions, or release PRs. | Do not replace Cocogitto for this task. The missing capability is binary distribution. |
| GoReleaser | Has a Rust builder using cargo-zigbuild and configurable build commands. | Lower fit. Its Rust documentation states that some build options are unsupported and Cargo workspaces may not work, depending on usage. |

Sources: [GitHub CLI release creation](https://cli.github.com/manual/gh_release_create), [upload action](https://github.com/taiki-e/upload-rust-binary-action), [dist introduction](https://axodotdev.github.io/cargo-dist/book/introduction.html), [cross](https://github.com/cross-rs/cross), [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild), [release-plz binary guidance](https://release-plz.dev/docs/extra/releasing-binaries), and [GoReleaser Rust support](https://goreleaser.com/customization/builds/builders/rust/).

Release-plz explicitly does not build or release binaries. Adding it would still require a separate distribution tool. GoReleaser's Rust builder is available in its open-source edition; the Pro restriction on that page concerns global hooks for Cargo package publication. That distinction prevents an incorrect rejection of its binary builder.

## Proposed integration with Cocogitto and git-cliff

Assign each operation to one owner:

| Operation | Owner |
| --- | --- |
| Calculate the next product version | Cocogitto |
| Update Cargo versions before the release commit | Existing cargo-release hook, installed through `mise` |
| Write `CHANGELOG.md` and render release notes | git-cliff |
| Create the release commit and `vX.Y.Z` tag | Cocogitto |
| Push the release commit and tag | Local `mise run release` task |
| Build, package, test, attest, and publish artifacts | GitHub Actions with repository `mise` tasks |
| Create and publish the GitHub release | One final CI job using `gh` |

Cocogitto supports commands before the version commit, which lets the existing hook update Cargo manifests. Keep `disable_changelog = true` so git-cliff remains the changelog owner. Avoid adding a second version calculator or tag creator. Sources: [Cocogitto bump hooks](https://docs.cocogitto.io/guide/bump.html) and [configuration reference](https://docs.cocogitto.io/reference/config).

git-cliff can render a future release with `--tag` without creating the Git tag. It requires an output option to write a file. Use an explicit `--output CHANGELOG.md` in the preparation hook. For CI notes, fetch full history and tags, render only the selected release, and write notes to a temporary file. Pass that file to `gh` with `--notes-file`. Its GitHub integration can add PR links and contributor data using a token from the environment. Sources: [git-cliff examples](https://git-cliff.org/docs/usage/examples/), [arguments](https://git-cliff.org/docs/usage/args), and [GitHub integration](https://git-cliff.org/docs/integration/github/).

The proposed CI sequence is:

1. Check out the pushed release tag. Confirm that the CLI package version and `oceans --version` match `X.Y.Z`.
2. Build only `oceans-cli` and its dependencies on each supported platform. Use the repository Rust version and `--locked` through `mise` tasks.
3. Test each binary on its target platform. Package the executable, license, and brief installation instructions.
4. Generate SHA-256 checksums and provenance attestations. Store the archives as workflow artifacts until every required build succeeds.
5. Wait for the existing image and Helm publication jobs. Download and check the complete CLI asset set in the final release job.
6. Create or recover a draft GitHub release for the existing tag with `--verify-tag`. Upload the assets and notes. Publish only after the asset checks pass.

This order removes the current race between tag CI and local GitHub release creation. A failed build leaves the public GitHub release unpublished. It cannot roll back container images or charts that other jobs already pushed; this is a completion gate, not an atomic transaction across registries.

GitHub recommends drafting first when release immutability is enabled, then attaching all assets before publication. This research did not establish whether Oceans enables that setting. Source: [GitHub release management](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository?tool=cli).

Keep retries safe. Check existing asset names and hashes before uploading again. Do not replace assets on a published release by default. `gh release upload --clobber` deletes the old file before uploading the replacement. The upload action uses that option in its current source. Sources: [GitHub CLI upload behavior](https://cli.github.com/manual/gh_release_upload) and [upload action source](https://github.com/taiki-e/upload-rust-binary-action/blob/main/main.sh).

## Where the upload action fits

The upload action is a valid alternative if maintainers want less packaging code. Set `bin: oceans`, select `oceans-cli` with its package input, set `locked: true`, choose an explicit target, and request SHA-256 checksums. Use archive names such as `oceans-vX.Y.Z-TARGET.tar.gz` or `.zip`.

For the proposed completion gate, use its `dry-run` mode to build and package, then collect those outputs as workflow artifacts. A final `gh` job can upload the complete set. The action runs Cargo internally and can install cross tools itself, so a fully repository-owned build path through `mise run` is clearer for this project. Sources: [action inputs](https://github.com/taiki-e/upload-rust-binary-action/blob/main/action.yml) and [implementation](https://github.com/taiki-e/upload-rust-binary-action/blob/main/main.sh).

## Where cargo-dist fits

cargo-dist becomes the preferred option if the requirement includes shell or PowerShell installers, Homebrew, or npm distribution. It supplies packaging, machine-readable manifests, checksums, linkage reports, and generated CI. It can also enable GitHub artifact attestations. Sources: [dist CI](https://axodotdev.github.io/cargo-dist/book/ci/index.html) and [configuration](https://axodotdev.github.io/cargo-dist/book/reference/config.html).

For Oceans, its adoption needs these explicit choices:

- Select `packages = ["oceans-cli"]` and set `precise-builds = true`. Package selection controls what ships; precise builds avoid compiling the wider gateway workspace.
- Keep Cocogitto as the version and tag owner. Dist accepts prepared versions and pushed tags; it does not require cargo-release to own release preparation.
- Resolve ownership of `.github/workflows/release.yml`. Dist normally generates and validates that file. Move image and chart jobs into reusable workflows, or use dist's build commands under repository-owned CI. Avoid disabling CI validation solely to patch generated YAML.
- Use one release creator. Either let dist create the GitHub release from the git-cliff changelog, or provide the draft that its `create-release = false` mode expects. The current local command creates a published release, so that mode is not a direct drop-in replacement.
- Test exact publication timing in the selected dist version. Dist normally hosts GitHub assets before package publication because installers and package managers need public download URLs. Draft asset URLs do not provide the same contract. A draft-until-complete flow is simpler while only archives are published.
- Pin the dist version and generated Action commits. Validate the generated Rust setup against the version selected by `mise`.

Sources: [dist customization](https://axodotdev.github.io/cargo-dist/book/ci/customizing.html), [configuration](https://axodotdev.github.io/cargo-dist/book/reference/config.html), [Rust quickstart](https://axodotdev.github.io/cargo-dist/book/quickstart/rust.html), and [release ordering explanation](https://github.com/axodotdev/cargo-dist/releases/tag/v0.17.0).

## Initial platform proposal

Start with Linux x86-64 and ARM64, macOS Intel and Apple Silicon, and Windows x86-64. Use explicit native runner architectures and target triples. This target set is proposed; no target builds were performed during this research.

| Platform | Initial build target |
| --- | --- |
| Linux x86-64 | `x86_64-unknown-linux-gnu` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| Windows x86-64 | `x86_64-pc-windows-msvc` |

For Linux, define and test the minimum glibc version. Building on an arbitrary current Ubuntu runner does not establish compatibility with older distributions. Add musl archives only after their dependency builds and runtime checks pass. cargo-zigbuild can target a glibc version, but its documentation lists cross-compilation limits. Source: [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild).

The CLI uses Rustls rather than default Reqwest TLS features, but that does not prove a static or portable binary. The shared bundle code uses capability-based file access. Verify upload, download, and installation behavior on Windows as well as Unix. Do not infer full platform support from a successful `--help` command.

## Baseline gaps to resolve during implementation

The checked-in release hooks deserve a focused preflight before adding distribution:

- `cog.toml` invokes `cargo release version`, but `cargo-release` is absent from the checked-in tool list. A maintainer environment may supply it today; make that dependency explicit through `mise`.
- The git-cliff hook does not specify a file output, and the checked-in configuration has no output setting. Make the changelog write explicit and verify that the version commit includes it.
- The runbook names `release-dry-run`, but the checked-in task list does not define it. Add a preview task or correct the runbook.
- `mise.toml` places Cargo output in shared cache paths. Packaging tasks must resolve the actual Cargo target directory or set a controlled task-specific directory. Do not assume the executable is in `./target/release`.

These are source observations, not reproduced failures of the release command. Do not run a version bump or publish a test release merely to investigate them.

## Acceptance evidence

Implementation should prove the full path before its first production release:

- A preview reports only the `oceans` executable and the intended target set.
- Hosted CI builds every supported target and runs the version and help checks.
- Tests on each supported operating system exercise Skills upload, download, and installation against a bounded fixture.
- Extracted archives run on clean target systems and meet the stated Linux compatibility baseline.
- Each archive has a matching checksum and verifiable provenance.
- A failed required job prevents final GitHub publication.
- A retry can complete a draft without changing the tag or replacing published assets.
- The published release contains the complete expected asset set, with CLI, images, and chart derived from the same tag.

Only research and current-state reads were completed here. Build compatibility, installer behavior, hosted release checks, signing, and publication remain untested.
