# Security policy

## Supported versions

| Version | Supported |
|---|---|
| 0.1.0 | yes — current release |
| older | no |

## Reporting a vulnerability

Do **not** open a public issue for security vulnerabilities.

Email the maintainers directly or use GitHub's private vulnerability reporting:
https://github.com/RobinMillford/Muvrix/security/advisories/new

Please include:

- A description of the vulnerability
- Steps to reproduce
- Impact assessment
- Suggested fix if you have one

We aim to acknowledge reports within 48 hours and provide a patch or mitigation within 14 days.

## Scope

This policy covers the Muvrix application source in this repository. Third-party components (libmpv, FFmpeg, etc.) are subject to their own disclosure processes.

## Release integrity

All release artifacts are published with `SHA256SUMS`. Verify downloads:

```sh
sha256sum -c SHA256SUMS
```

If the project publishes signed manifests in the future, verify signatures before installing.
