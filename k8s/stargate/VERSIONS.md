# Chart support image pins

The Helm connection-test Pod uses the following immutable multi-platform image.
The machine-readable record is [versions.json](versions.json).

| Image | Tag | Index digest |
| --- | --- | --- |
| docker.io/library/busybox | 1.36.1 | `sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662` |

Verified from the Docker Hub index on 2026-11-09, including linux/amd64 and
linux/arm64. Update the template, this file and versions.json together when
changing the test image. Stargate's release version is in Chart.yaml/Cargo.toml;
each deployment pins the released server image and chart in its own repository.
