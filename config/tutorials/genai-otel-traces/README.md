# GenAI OpenTelemetry traces

`genai_agent.py` simulates an LLM agent and sends traces following the
[OpenTelemetry GenAI semantic conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/)
to Quickwit over OTLP/gRPC. No LLM provider is called, so no API key is needed.

```bash
pip install -r requirements.txt
python genai_agent.py --endpoint http://localhost:7281 --num-conversations 200
```

See the [GenAI traces tutorial](../../../docs/distributed-tracing/send-traces/genai-traces.md)
for the queries and aggregations to run on the indexed spans.
