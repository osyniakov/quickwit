---
title: GenAI traces (LLM & agents)
description: Send OpenTelemetry GenAI traces from an LLM agent to Quickwit and analyze token usage, latency and errors.
tags: [otel, genai, llm, traces, ingestion]
sidebar_position: 3
---

The OpenTelemetry [GenAI semantic conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/) describe how to trace calls to LLMs, agents and tools. Instrumentation libraries such as [OpenLLMetry](https://github.com/traceloop/openllmetry), [OpenLIT](https://github.com/openlit/openlit) or the [OpenTelemetry Python contrib instrumentations](https://github.com/open-telemetry/opentelemetry-python-contrib/tree/main/instrumentation-genai) emit these spans for OpenAI, Anthropic, Bedrock, LangChain and more.

Because GenAI spans are regular OpenTelemetry spans, Quickwit indexes them out of the box. In this tutorial, we send simulated traces of an LLM agent to Quickwit and use the search and aggregation APIs to answer typical questions: how many tokens does each model consume, how slow are the model calls, and which calls fail?

## Prerequisites

- Python 3 installed
- A running Quickwit instance: [install Quickwit](/docs/get-started/installation.md) and run `./quickwit run`

## The example agent

The script [`genai_agent.py`](https://github.com/quickwit-oss/quickwit/tree/main/config/tutorials/genai-otel-traces/genai_agent.py) simulates a travel assistant agent. It does not call any LLM provider, so no API key is required. Each conversation produces one trace:

```
invoke_agent travel-assistant           (INTERNAL)
├── chat gpt-4o-mini                    (CLIENT, finish_reasons=["tool_calls"])
├── execute_tool get_weather            (INTERNAL)
└── chat gpt-4o-mini                    (CLIENT, finish_reasons=["stop"])
```

The spans carry the GenAI attributes, for instance for a `chat` span:

| Attribute | Example |
|-----------|---------|
| `gen_ai.operation.name` | `chat` |
| `gen_ai.provider.name` | `openai` |
| `gen_ai.request.model` | `gpt-4o-mini` |
| `gen_ai.response.model` | `gpt-4o-mini-2024-07-18` |
| `gen_ai.response.finish_reasons` | `["tool_calls"]` |
| `gen_ai.usage.input_tokens` | `812` |
| `gen_ai.usage.output_tokens` | `95` |
| `gen_ai.input.messages` / `gen_ai.output.messages` | Prompt and completion, as JSON strings |
| `error.type` | `rate_limit_exceeded`, set when the call fails |

About 5% of the model calls fail, so we have errors to look for.

## Send the traces to Quickwit

Quickwit receives OTLP traces over gRPC on port `7281`. Install the dependencies and run the script:

```bash
cd config/tutorials/genai-otel-traces
pip install -r requirements.txt
python genai_agent.py --endpoint http://localhost:7281 --num-conversations 200
```

The spans are indexed in the `otel-traces-v0_9` index, which Quickwit creates automatically. Its doc mapping is described in the [OpenTelemetry service](../otel-service.md) page: GenAI attributes end up in the `span_attributes` JSON field, so `gen_ai.usage.input_tokens` is available as `span_attributes.gen_ai.usage.input_tokens`.

:::caution
Prompts, completions and tool arguments can contain sensitive data. The GenAI conventions make them opt-in, and most instrumentation libraries only record them when explicitly enabled. Run the script with `--no-content` to leave them out.
:::

Wait about 10 seconds for the spans to be committed, then let's explore them.

## Search GenAI spans

Find the failed model calls and see why they failed:

```bash
curl -XPOST http://localhost:7280/api/v1/otel-traces-v0_9/search -H 'Content-Type: application/json' -d '{
    "query": "span_attributes.gen_ai.operation.name:chat AND span_attributes.error.type:*",
    "max_hits": 5
}' | jq '.hits[] | {trace_id, span_name, error: .span_attributes["error.type"]}'
```

Find all the traces in which the agent called a given tool:

```bash
curl -XPOST http://localhost:7280/api/v1/otel-traces-v0_9/search -H 'Content-Type: application/json' -d '{
    "query": "span_attributes.gen_ai.tool.name:search_flights",
    "max_hits": 5
}' | jq '.hits[].trace_id'
```

Attribute values are indexed with the `raw` tokenizer, so a query matches the exact value only: full-text search inside prompts and completions is not available in the default index.

## Analyze token usage and latency

Token usage and latency per model, with the 95th percentile latency of `chat` calls:

```bash
curl -XPOST http://localhost:7280/api/v1/otel-traces-v0_9/search -H 'Content-Type: application/json' -d '{
    "query": "span_attributes.gen_ai.operation.name:chat",
    "max_hits": 0,
    "aggs": {
        "models": {
            "terms": { "field": "span_attributes.gen_ai.request.model" },
            "aggs": {
                "input_tokens": { "sum": { "field": "span_attributes.gen_ai.usage.input_tokens" } },
                "output_tokens": { "sum": { "field": "span_attributes.gen_ai.usage.output_tokens" } },
                "latency_millis": { "percentiles": { "field": "span_duration_millis", "percents": [50, 95] } }
            }
        }
    }
}' | jq '.aggregations.models.buckets'
```

Each bucket looks like:

```json
{
  "key": "claude-sonnet-4-5",
  "doc_count": 113,
  "input_tokens": { "value": 88005.0 },
  "output_tokens": { "value": 21779.0 },
  "latency_millis": { "values": { "50.0": 2368.97, "95.0": 3605.53 } }
}
```

Errors by provider and error type:

```bash
curl -XPOST http://localhost:7280/api/v1/otel-traces-v0_9/search -H 'Content-Type: application/json' -d '{
    "query": "span_attributes.gen_ai.operation.name:chat AND span_attributes.error.type:*",
    "max_hits": 0,
    "aggs": {
        "providers": {
            "terms": { "field": "span_attributes.gen_ai.provider.name" },
            "aggs": {
                "error_types": { "terms": { "field": "span_attributes.error.type" } }
            }
        }
    }
}' | jq '.aggregations.providers.buckets'
```

Tokens consumed over time, in 10 minute buckets:

```bash
curl -XPOST http://localhost:7280/api/v1/otel-traces-v0_9/search -H 'Content-Type: application/json' -d '{
    "query": "span_attributes.gen_ai.operation.name:chat",
    "max_hits": 0,
    "aggs": {
        "over_time": {
            "date_histogram": { "field": "span_start_timestamp_nanos", "fixed_interval": "10m" },
            "aggs": {
                "output_tokens": { "sum": { "field": "span_attributes.gen_ai.usage.output_tokens" } }
            }
        }
    }
}' | jq '.aggregations.over_time.buckets'
```

## Visualize the traces

The traces of the agent can be explored in Jaeger, see [Plug Quickwit to Jaeger](../plug-quickwit-to-jaeger.md), and the aggregations above can be turned into dashboards with [Grafana](/docs/get-started/tutorials/trace-analytics-with-grafana.md).

## Instrument a real application

To trace a real application, replace the simulated agent with an instrumentation library and point its OTLP exporter to Quickwit. For instance, with the OpenAI instrumentation from OpenTelemetry Python contrib:

```bash
pip install opentelemetry-distro opentelemetry-exporter-otlp opentelemetry-instrumentation-openai-v2

OTEL_SERVICE_NAME=my-llm-app \
OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=http://localhost:7281 \
OTEL_EXPORTER_OTLP_TRACES_PROTOCOL=grpc \
OTEL_METRICS_EXPORTER=none \
OTEL_LOGS_EXPORTER=none \
opentelemetry-instrument python my_llm_app.py
```

Set `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT=true` to also record prompts and completions.
