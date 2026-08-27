# Export Traces and Metrics

`See also`: [Observability and Request Logs](../observability-and-request-logs.md), [Request Logs](request-logs.md), [Kubernetes and Helm](../../setup/kubernetes-and-helm.md), [Configuration Reference](../../configuration/configuration-reference.md), [Admin Runbooks](../operator-runbooks.md)

Oceans LLM sends traces and metrics with the OpenTelemetry Protocol (OTLP). Admins can send this data to an OpenTelemetry Collector or to a Datadog Agent that accepts OTLP.

The gateway uses OTLP over gRPC. The examples use receiver port `4317`. You can set a different port in the endpoint URI. The Helm chart does not install a collector or an agent.

## Understand the Data Flow

The gateway creates telemetry and sends each configured signal to its OTLP receiver:

```text
Oceans LLM gateway -- traces --> OTLP/gRPC receiver -- traces --> backend
                   -- metrics -> OTLP/gRPC receiver -- metrics -> backend
```

Traces and metrics can use the same receiver or separate receivers.

The receiver can be:

- an OpenTelemetry Collector `Service`
- an OpenTelemetry Collector sidecar
- a node-local collector or vendor agent
- a Datadog Agent `Service` backed by a DaemonSet

OTLP export is separate from request-log storage. The gateway can store request logs when no OTLP receiver is present. Admins and users can still view allowed request logs in the admin UI. See [Observability and Request Logs](../observability-and-request-logs.md) for payload policy and retention.

## Prerequisites

Before you configure the gateway:

1. Deploy an OTLP receiver that accepts gRPC traffic.
2. Make the receiver address reachable from the gateway process or pod.
3. Allow TCP traffic from the gateway to the receiver port.
4. Configure the receiver to send data to its backend.

The current gateway exporter supports plaintext OTLP/gRPC endpoints. Use an `http://` URI on a trusted network. For secure external export, send plaintext traffic to a local or in-cluster collector and configure that collector to use TLS when it sends data to the external backend.

## Configure OTLP Export

Add the receiver endpoints to the `server` section of `gateway.yaml`:

```yaml
server:
  otel_endpoint: http://otel-collector.observability.svc:4317
  otel_metrics_endpoint: http://otel-collector.observability.svc:4317
  otel_trace_sample_ratio: 1.0
  otel_export_interval_secs: 30
```

The fields have these effects:

| Field | Effect |
| --- | --- |
| `otel_endpoint` | Enables trace export and sets the OTLP/gRPC trace endpoint. |
| `otel_metrics_endpoint` | Enables metric export and sets the OTLP/gRPC metric endpoint. If absent, metrics use `otel_endpoint`. |
| `otel_trace_sample_ratio` | Samples root traces from `0.0` through `1.0`. The default is `1.0`. An upstream parent decision is kept. |
| `otel_export_interval_secs` | Sets the interval for metric export batches. |

Trace sampling does not sample metrics. A value of `0.5` keeps about half of new root traces. All metric instruments remain active.

The gateway uses parent-based head sampling. If you need to retain all errors, slow requests, client cancellations, or budget failures while you reduce normal trace volume, configure tail sampling in the OpenTelemetry Collector. The collector must receive a trace before it can make that final-outcome decision.

Restart the gateway after you change these values. An invalid endpoint URI stops config validation. A sampling ratio outside `0.0` through `1.0` also stops validation.

## Send Data to an OpenTelemetry Collector

Point both endpoints at the collector OTLP/gRPC receiver. The collector can send traces and metrics to different backends after it receives them.

For Kubernetes, use the collector `Service` DNS name:

```yaml
gateway:
  config:
    server:
      otel_endpoint: http://otel-collector.observability.svc:4317
      otel_metrics_endpoint: http://otel-collector.observability.svc:4317
      otel_trace_sample_ratio: 1.0
      otel_export_interval_secs: 30
```

If the collector runs as a sidecar, use `http://127.0.0.1:4317`. The checked-in [sidecar values example](../../../deploy/helm/oceans-llm/examples/observability-sidecar-values.yaml) shows the pod wiring. You must also create the referenced collector `ConfigMap`.

## Send Data to Datadog

Enable the OTLP/gRPC receiver on the existing Datadog Agent before you update the gateway. Expose that receiver through an in-cluster `Service` that the gateway pods can reach.

Then point the gateway at the Agent:

```yaml
gateway:
  config:
    server:
      otel_endpoint: http://datadog-agent.monitoring.svc.cluster.local:4317
      otel_metrics_endpoint: http://datadog-agent.monitoring.svc.cluster.local:4317
      otel_trace_sample_ratio: 0.5
      otel_export_interval_secs: 30

observability:
  podLabels:
    tags.datadoghq.com/service: oceans-llm-gateway
    tags.datadoghq.com/env: production
```

Use [observability-datadog-values.yaml](../../../deploy/helm/oceans-llm/examples/observability-datadog-values.yaml) as a complete Helm values example. Replace its namespace, `Service` name, database secret, and environment label for your cluster.

For a Datadog Agent DaemonSet, route each gateway pod to an Agent on the same node when possible. A `Service` with `internalTrafficPolicy: Local` can provide this route. Confirm that every node that can run the gateway also runs an Agent pod.

Set the trace ratio in Oceans LLM when the Agent is shared with other services. An Agent-wide sampler can change trace intake for every service that sends data to that Agent. The gateway ratio changes only new root traces from Oceans LLM.

The gateway sets these OpenTelemetry resource attributes:

- `service.name=oceans-llm-gateway`
- `service.namespace=oceans-llm`
- `service.version=<gateway version>`

The Datadog pod labels use the same service name so Datadog can join the telemetry under one service identity.

## Verify the Export

After the gateway restarts:

1. Check the gateway startup logs for OTLP exporter errors.
2. Send a normal model request through the gateway.
3. Wait at least one configured metric export interval.
4. Search the backend for the service name `oceans-llm-gateway`.
5. Confirm that a trace and at least one gateway metric are present.

Gateway metric names include:

- `gateway.chat.requests`
- `gateway.chat.request.duration`
- `gateway.chat.tokens`
- `gateway.chat.cost.usd`
- `gateway.mcp.tool_invocations`

For a streaming model request, confirm that the trace contains `http.server.request`, `gateway.provider.operation`, `http.client.request`, and `gateway.provider.stream`. The stream span reports first-chunk and first-output latency, chunk and byte totals, terminal-event presence, and its termination reason. Model access, routing, budget, usage, ledger, and request-log spans appear when the request reaches those phases.

Trace attributes do not contain prompt content, tool arguments, credentials, full request headers, or URL query values. Use the governed request-log store when you need captured payload data.

For Datadog, check APM for the `oceans-llm-gateway` service. Use Metrics Explorer to find the gateway metrics. Datadog can map OpenTelemetry metric names during intake. Inspect the received names if an exact search returns no result.

If request logs appear in the admin UI but OTLP data does not appear in the backend, check the receiver address, network policy, receiver logs, and backend exporter settings. The [Missing OTLP Collector](../operator-runbooks.md#missing-otlp-collector) runbook provides more checks.

## Disable OTLP Export

Remove `otel_endpoint` and `otel_metrics_endpoint`, then restart the gateway. This stops OTLP trace and metric export. It does not disable structured process logs or database-backed request logs.
