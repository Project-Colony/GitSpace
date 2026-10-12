# Security Policy

## Supported versions

Only the **latest release** receives security fixes. GitSpace ships as a single
binary, through Colony and the release page, so the fix for a vulnerability is
the next release, not a patch to an older one.

## Reporting a vulnerability

Please report vulnerabilities **privately** via
[GitHub Security Advisories](https://github.com/Project-Colony/GitSpace/security/advisories/new)
("Report a vulnerability"). Do not open a public issue for exploitable bugs.

Include the version (`gitspace --version`), the platform, and the steps that
reproduce it. The report stays private until a fixed release is out.

## What is in scope

GitSpace holds access tokens for GitHub and GitLab and talks to remote servers
on your behalf. Reports of particular interest:

- a token sent to a host it does not belong to, or over plain HTTP;
- a token readable from disk or from the logs by anyone but you;
- the OAuth sign-in on the loopback callback (`127.0.0.1`, ports 9876 to 9886)
  accepting a forged or replayed response;
- a repository, remote or path that makes GitSpace run a command or write a
  file outside what you asked for;
- a release asset whose signature or `.meta` sidecar does not verify (see
  [Code signing policy](README.md#code-signing-policy)).
