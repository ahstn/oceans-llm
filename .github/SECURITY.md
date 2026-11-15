# Security

Oceans LLM handles API keys, provider credentials, request data, and access to
administrative operations and tools. Please report suspected vulnerabilities
privately so they can be investigated before details are disclosed.

## Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability. Use [GitHub's
private vulnerability reporting](https://github.com/ahstn/oceans-llm/security/advisories/new)
to send a report to the maintainers.

Include what you found, the affected Oceans version, the expected and actual
behavior, and steps to reproduce it. A minimal proof of concept and any relevant
configuration or logs can help. Remove credentials and other sensitive data
from your report.

The maintainers will review the report and coordinate disclosure with you. There
is no fixed response deadline. Reporters will be credited with their consent.

## Supported versions

Security fixes are made for the latest release. If you are using an older
version, please include it in your report and check whether the issue also
affects the latest release when practical.

## In scope

Examples of issues we want to hear about include:

- Authentication, authorization, or isolation failures in the gateway or admin
  control plane.
- Exposure of API keys, provider credentials, request data, or other sensitive
  information.
- Bypasses of access controls for models, MCP servers, or tools.
- Vulnerabilities in Oceans deployment artifacts or release processes.
