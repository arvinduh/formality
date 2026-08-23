# Security Policy

The `formality` maintainers and community take security issues seriously. We
appreciate your efforts to responsibly disclose vulnerabilities.

---

## Supported Versions

We provide security updates for the following versions of `formality`:

| Version        | Supported          |
| :------------- | :----------------- |
| Latest Release | :white_check_mark: |
| `main` branch  | :white_check_mark: |
| Older releases | :x:                |

We strongly recommend always running the latest version of `fml`.

---

## Reporting a Vulnerability

**Please do NOT report security vulnerabilities through public GitHub issues.**

If you discover a security vulnerability in `formality` (such as malicious code
execution via tool invocation, unsafe file operations during `fml sync` or
`fml install`, or credential leaks), please report it via one of the following
methods:

1. **GitHub Private Vulnerability Reporting** (Preferred): Navigate to the
   [Formality Security tab](https://github.com/arvinduh/formality/security/advisories)
   on GitHub and click **"Report a vulnerability"**.

2. **Email Security Contact**: If private vulnerability reporting is
   unavailable, send an email describing the issue, potential impact, and
   reproduction steps to the project maintainers.

### What to Include in your Report

- A detailed description of the vulnerability and its potential security impact.
- Step-by-step instructions or proof-of-concept (PoC) code to reproduce the
  issue.
- The affected component, command (`fmt`, `lint`, `sync`, `install`), or
  surface.
- Any suggested mitigations or patches, if available.

---

## Response & Resolution Policy

- **Acknowledgment**: We aim to acknowledge receipt of security reports within
  **48 hours**.
- **Assessment**: The maintenance team will investigate and determine the
  severity and scope of the reported vulnerability.
- **Fix & Patch**: A fix will be developed in a private security branch and
  merged once verified.
- **Disclosure**: Once a fix is released in a new binary release, a public
  Security Advisory will be published detailing the vulnerability and crediting
  the reporter (unless requested otherwise).
