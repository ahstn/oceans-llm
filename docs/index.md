---
layout: home
title: Oceans LLM Documentation
sidebar: false
hero:
  name: Oceans LLM
  text: Route, govern, and observe AI traffic.
  tagline: Operate one policy-aware gateway for provider routing, identity, spend controls, and request observability.
  image:
    src: /images/oceans-logo-rounded-square.png
    alt: Oceans LLM wave gateway illustration
  actions:
    - theme: brand
      text: Getting Started
      link: /getting-started
    - theme: alt
      text: Runtime Setup
      link: /setup/runtime-bootstrap-and-access
    - theme: alt
      text: Helm Chart
      link: /setup/kubernetes-and-helm
features:
  - title: Run the gateway
    details: Bootstrap local access, seeded admin identity, API keys, and deploy-time runtime shape.
    link: /getting-started
    linkText: Start here
  - title: Deploy to Kubernetes
    details: Install the OCI Helm chart, wire PostgreSQL secrets, run hook Jobs, and expose traffic through the gateway.
    link: /setup/kubernetes-and-helm
    linkText: Deploy
  - title: Configure routing
    details: Shape providers, aliases, pricing inputs, auth modes, and request behavior from config.
    link: /configuration/configuration-reference
    linkText: Configure
  - title: Govern access
    details: Understand admin control-plane roles, SSO state, team boundaries, API-key ownership, and service-account-style callers.
    link: /access/identity-and-access
    linkText: Review access
  - title: Control spend
    details: Set user, service-account, and user model budgets, then review operator accounting behavior.
    link: /access/budgets
    linkText: Manage budgets
  - title: Trace requests
    details: Export traces and metrics through OTLP, then inspect request logs, MCP invocations, payload state, and failure details.
    link: /operations/observability/export-traces-and-metrics
    linkText: Inspect observability
---
