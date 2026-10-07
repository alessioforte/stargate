# Releasing Stargate

Releases publish an image and Helm chart to GHCR and create a GitHub Release in
this repository. Each environment pins its version in its own GitOps/infra repo;
this workflow never deploys or writes to other repositories.

## Cut a release

1. Set `[workspace.package].version` in `Cargo.toml` to the new version and keep
   the root `[package].version` equal to it. Member crates inherit the workspace
   version. Refresh `Cargo.lock` with `cargo check --workspace --features cluster`,
   commit the version changes, and merge to `main` after CI passes.
2. Run a dry run on `main`:
   `gh workflow run release.yml --ref main -f dry_run=true`. It uses the workspace
   version, runs the existing edge/cluster CI, builds the image, and lints,
   packages, and renders the chart. It skips registry logins, publication guards,
   image/chart pushes, and GitHub Release creation, so published versions can
   still be validated. Build caches may be updated.
3. Tag the intended commit and push the tag, for example:

   ```sh
   git switch main
   git pull --ff-only
   git tag -a v1.5.0 -m 'Stargate 1.5.0'
   git push origin v1.5.0
   ```

Use SemVer `X.Y.Z` or `X.Y.Z-prerelease`, such as `1.5.0-rc.1`. The tag must be
`v` plus the exact workspace version; prereleases are marked on GitHub. Build
metadata (`+...`) is unsupported because Docker tags cannot contain `+`.
Manual publishing (`dry_run=false`) requires selecting a matching tag ref.
The chart's packaged `version` and `appVersion` are both the release version;
the source `Chart.yaml` is not changed by the workflow.

The release image is built with `STARGATE_PROFILE=cluster` and includes the
Console, Auth UI, and mail templates. One version tag covers both `linux/amd64`
and `linux/arm64`; `PLATFORMS` in the release workflow is the single architecture
setting. QEMU enables the ARM64 build on the GitHub-hosted AMD64 runner.
Configure the chart with `deployment.mode=cluster`, `persistence.enabled=false`,
and PostgreSQL/Redis settings through `runtimeConfig` and `runtimeSecret` (see
the [chart guide](../k8s/stargate/README.md)). The chart's default edge values
require an edge image and must be overridden for this release image.

Only the publishing job gets `contents: write` and `packages: write`, using
`GITHUB_TOKEN`. Existing GHCR packages must grant this repository Actions access.
For forks, update the chart's default `image.repository` to the lowercase owner;
the workflow checks that the packaged chart renders the expected image.

## Verify artifacts

For `VERSION=1.5.0` and `OWNER=alessioforte` (lowercase):

```sh
gh release view "v$VERSION" --repo "$OWNER/stargate"
docker buildx imagetools inspect "ghcr.io/$OWNER/stargate:$VERSION"
docker buildx imagetools inspect "ghcr.io/$OWNER/stargate:$VERSION" --format '{{json .Provenance}}'
docker buildx imagetools inspect "ghcr.io/$OWNER/stargate:$VERSION" --format '{{json .SBOM}}'
docker pull "ghcr.io/$OWNER/stargate:$VERSION"
docker image inspect "ghcr.io/$OWNER/stargate:$VERSION" --format '{{json .Config.Labels}}'
docker image inspect "ghcr.io/$OWNER/stargate:$VERSION" --format '{{json .Config.Env}}'
helm show chart "oci://ghcr.io/$OWNER/charts/stargate" --version "$VERSION"
helm template stargate "oci://ghcr.io/$OWNER/charts/stargate" --version "$VERSION"
```

The release body contains the image tag **and digest**, plus the chart reference
and version. Confirm the digest matches the registry, the four OCI labels are
present, both AMD64 and ARM64 manifests have provenance and SBOM, the image
environment includes `STARGATE_RUNTIME_PROFILE=cluster`, and chart
`version`/`appVersion` match. Without overriding `image.tag`, the chart must render
`ghcr.io/<owner>/stargate:<VERSION>`. Authenticate Docker and Helm to GHCR first
if the packages are private.

Versions are immutable: both image and chart existence are checked before
either is published; registry/authentication errors stop the release. There is
no `latest` image tag. Release runs for the same ref are serialized and never
cancelled. GHCR does not provide an atomic image-and-chart transaction: if a run
fails after one artifact is published, rerunning refuses that version. Inspect
the failure and cut a new version. Publish through this workflow so its
concurrency and preflight checks apply.

Possible follow-ups: image signing with cosign, package cleanup, separate edge
artifacts, and native builders for faster builds.
