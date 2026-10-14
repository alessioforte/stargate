# Security Policy

Stargate is an identity provider and API gateway, so security reports are
treated as the highest-priority work in the project. Thank you for helping keep
Stargate and its users safe.

## Project status

Stargate is **alpha** software. It has not had an independent security audit,
and configuration formats and APIs may still change between minor versions.
Evaluate it carefully before putting it in front of production traffic or
sensitive identities.

## Supported versions

Until 1.0, only the latest released minor version receives security fixes.

| Version           | Supported          |
| ----------------- | ------------------ |
| latest `0.x`      | :white_check_mark: |
| older `0.x`       | :x:                |
| unreleased `main` | best effort        |

## Reporting a vulnerability

**Do not open a public issue, discussion, or pull request for security
problems.**

Report privately through one of these channels:

1. **GitHub private vulnerability reporting (preferred):** open
   [a new security advisory](https://github.com/alessioforte/stargate/security/advisories/new)
   from the repository's **Security** tab.
2. **Email:** [fortealessio@gmail.com](mailto:fortealessio@gmail.com) with
   `[stargate security]` in the subject.

Please include:

- the affected version, commit, or image digest, and the build profile
  (`edge` or `cluster`);
- the component involved (for example OAuth/OIDC provider, login/MFA, admin API,
  gateway routing or policies, Console, Helm chart);
- reproduction steps or a proof of concept, and the relevant configuration with
  secrets removed;
- the impact you observed and any suggested fix.

## What to expect

- Acknowledgement within **5 business days**.
- An initial assessment, including severity and whether the report is accepted,
  within **14 days**.
- A fix or mitigation as fast as the severity requires; the target for accepted
  reports is **90 days** or less.
- Coordinated disclosure: a GitHub Security Advisory (with a CVE when
  applicable) is published once a fixed release is available. Reporters are
  credited unless they prefer to stay anonymous.

Stargate is maintained by a single person; these are targets, not contractual
guarantees. If you do not receive an acknowledgement, follow up through the
other channel.

## Scope

In scope:

- the Stargate server (IAM, OAuth 2.0/OpenID Connect provider, gateway, CLI);
- the Console, hosted auth UI, and transactional mail templates in `apps/`;
- the published container images, Dockerfile, Helm chart in `k8s/`, and the
  example `docker-compose.yml`.

Out of scope:

- vulnerabilities in third-party dependencies that do not affect Stargate in
  practice (please report them upstream; reports showing a reachable impact in
  Stargate are welcome);
- volumetric denial of service, and findings that require an already
  compromised host, database, or super-admin account;
- the example credentials and placeholder secrets in `docker-compose.yml` and
  `.env.example`, which are documented as local-only defaults;
- missing hardening that is already documented as a deployment responsibility
  (for example TLS termination, `TRUSTED_PROXIES`, `CORS_ORIGINS`).

## Safe harbor

Good-faith research that respects this policy is welcome. Test only against
instances you own or are authorized to test, avoid accessing or modifying other
people's data, and give us reasonable time to ship a fix before disclosing.
