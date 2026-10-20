"""Emits OpenTelemetry traces that follow the GenAI semantic conventions.

The script simulates a "travel assistant" agent answering user questions. Each
conversation produces a trace made of:

    invoke_agent travel-assistant           (INTERNAL)
    ├── chat <model>                        (CLIENT, finish_reasons=["tool_calls"])
    ├── execute_tool <tool>                 (INTERNAL)
    └── chat <model>                        (CLIENT, finish_reasons=["stop"])

No LLM provider is called: model responses, latencies, token counts and
failures are simulated, so the example runs without any API key. Spans are
exported over OTLP/gRPC to Quickwit, which indexes them into the
`otel-traces-v0_9` index by default.

Attribute names follow the OpenTelemetry GenAI semantic conventions:
https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/
"""

import argparse
import json
import random
import time
import uuid

from opentelemetry import trace
from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk.resources import Resource
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor
from opentelemetry.trace import SpanKind, Status, StatusCode

AGENT_NAME = "travel-assistant"

# (provider name, request model, response model, server address)
MODELS = [
    ("openai", "gpt-4o-mini", "gpt-4o-mini-2024-07-18", "api.openai.com"),
    ("openai", "gpt-4o", "gpt-4o-2024-08-06", "api.openai.com"),
    ("anthropic", "claude-sonnet-4-5", "claude-sonnet-4-5-20250929", "api.anthropic.com"),
    ("gcp.gemini", "gemini-2.5-flash", "gemini-2.5-flash", "generativelanguage.googleapis.com"),
]

# (user question, tool name, tool arguments, tool result, final answer)
SCENARIOS = [
    (
        "What's the weather in Paris tomorrow?",
        "get_weather",
        {"city": "Paris"},
        {"forecast": "sunny", "temperature_c": 24},
        "Tomorrow in Paris it will be sunny with a high of 24°C.",
    ),
    (
        "Find me a flight from Boston to Lisbon next Friday.",
        "search_flights",
        {"origin": "BOS", "destination": "LIS", "date": "next friday"},
        {"flights": [{"number": "TP218", "price_usd": 642}]},
        "TAP flight TP218 leaves Boston on Friday evening for $642.",
    ),
    (
        "Book a hotel near the Colosseum for two nights.",
        "search_hotels",
        {"city": "Rome", "near": "Colosseum", "nights": 2},
        {"hotels": [{"name": "Hotel Lupa", "price_per_night_eur": 180}]},
        "Hotel Lupa is a 5 minute walk from the Colosseum at 180€ per night.",
    ),
    (
        "How much is 250 USD in yen?",
        "convert_currency",
        {"amount": 250, "from": "USD", "to": "JPY"},
        {"amount": 37125.0},
        "250 USD is about 37,125 JPY.",
    ),
]

# Probability that a chat call fails, simulating provider-side errors.
CHAT_ERROR_RATE = 0.05
CHAT_ERRORS = ["rate_limit_exceeded", "timeout", "context_length_exceeded"]

SYSTEM_INSTRUCTIONS = "You are a helpful travel assistant."


def to_nanos(seconds: float) -> int:
    return int(seconds * 1_000_000_000)


def chat_span(tracer, model, start_secs, input_messages, output_message, finish_reason, capture_content):
    """Records one `chat` call to an LLM and returns its end time in seconds.

    Raises `RuntimeError` when the simulated provider call fails, after
    recording the error on the span.
    """
    provider_name, request_model, response_model, server_address = model
    duration_secs = random.uniform(0.3, 4.0)
    input_tokens = random.randint(150, 1_500)
    output_tokens = random.randint(20, 400)

    span = tracer.start_span(
        f"chat {request_model}",
        kind=SpanKind.CLIENT,
        start_time=to_nanos(start_secs),
        attributes={
            "gen_ai.operation.name": "chat",
            "gen_ai.provider.name": provider_name,
            "gen_ai.request.model": request_model,
            "gen_ai.request.temperature": 0.2,
            "gen_ai.request.max_tokens": 1024,
            "server.address": server_address,
            "server.port": 443,
        },
    )
    if capture_content:
        span.set_attribute("gen_ai.system_instructions", json.dumps([{"type": "text", "content": SYSTEM_INSTRUCTIONS}]))
        span.set_attribute("gen_ai.input.messages", json.dumps(input_messages))

    if random.random() < CHAT_ERROR_RATE:
        error_type = random.choice(CHAT_ERRORS)
        span.set_attribute("error.type", error_type)
        span.set_status(Status(StatusCode.ERROR, f"provider returned {error_type}"))
        span.end(end_time=to_nanos(start_secs + duration_secs))
        raise RuntimeError(error_type)

    span.set_attribute("gen_ai.response.id", f"resp_{uuid.uuid4().hex[:24]}")
    span.set_attribute("gen_ai.response.model", response_model)
    span.set_attribute("gen_ai.response.finish_reasons", [finish_reason])
    span.set_attribute("gen_ai.usage.input_tokens", input_tokens)
    span.set_attribute("gen_ai.usage.output_tokens", output_tokens)
    if capture_content:
        span.set_attribute("gen_ai.output.messages", json.dumps([output_message]))
    end_secs = start_secs + duration_secs
    span.end(end_time=to_nanos(end_secs))
    return end_secs


def tool_span(tracer, tool_name, tool_call_id, tool_args, tool_result, start_secs, capture_content):
    """Records the execution of a tool requested by the model and returns its end time."""
    end_secs = start_secs + random.uniform(0.05, 0.8)
    span = tracer.start_span(
        f"execute_tool {tool_name}",
        kind=SpanKind.INTERNAL,
        start_time=to_nanos(start_secs),
        attributes={
            "gen_ai.operation.name": "execute_tool",
            "gen_ai.tool.name": tool_name,
            "gen_ai.tool.call.id": tool_call_id,
            "gen_ai.tool.type": "function",
        },
    )
    if capture_content:
        span.set_attribute("gen_ai.tool.call.arguments", json.dumps(tool_args))
        span.set_attribute("gen_ai.tool.call.result", json.dumps(tool_result))
    span.end(end_time=to_nanos(end_secs))
    return end_secs


def run_conversation(tracer, start_secs, capture_content):
    """Records one agent invocation: model call, tool call, then final model call."""
    model = random.choice(MODELS)
    question, tool_name, tool_args, tool_result, answer = random.choice(SCENARIOS)
    tool_call_id = f"call_{uuid.uuid4().hex[:16]}"

    agent_span = tracer.start_span(
        f"invoke_agent {AGENT_NAME}",
        kind=SpanKind.INTERNAL,
        start_time=to_nanos(start_secs),
        attributes={
            "gen_ai.operation.name": "invoke_agent",
            "gen_ai.provider.name": model[0],
            "gen_ai.agent.name": AGENT_NAME,
            "gen_ai.agent.id": "agent_travel_001",
            "gen_ai.request.model": model[1],
            "gen_ai.conversation.id": f"conv_{uuid.uuid4().hex[:16]}",
        },
    )
    user_message = {"role": "user", "parts": [{"type": "text", "content": question}]}
    tool_call_message = {
        "role": "assistant",
        "parts": [{"type": "tool_call", "id": tool_call_id, "name": tool_name, "arguments": tool_args}],
        "finish_reason": "tool_calls",
    }
    tool_response_message = {
        "role": "tool",
        "parts": [{"type": "tool_call_response", "id": tool_call_id, "response": tool_result}],
    }
    answer_message = {
        "role": "assistant",
        "parts": [{"type": "text", "content": answer}],
        "finish_reason": "stop",
    }
    end_secs = start_secs
    with trace.use_span(agent_span, end_on_exit=False):
        try:
            end_secs = chat_span(
                tracer, model, end_secs, [user_message], tool_call_message, "tool_calls", capture_content
            )
            end_secs = tool_span(
                tracer, tool_name, tool_call_id, tool_args, tool_result, end_secs, capture_content
            )
            end_secs = chat_span(
                tracer,
                model,
                end_secs,
                [user_message, tool_call_message, tool_response_message],
                answer_message,
                "stop",
                capture_content,
            )
        except RuntimeError as error:
            agent_span.set_attribute("error.type", str(error))
            agent_span.set_status(Status(StatusCode.ERROR, f"model call failed: {error}"))
    agent_span.end(end_time=to_nanos(end_secs + 0.01))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--endpoint", default="http://localhost:7281", help="OTLP/gRPC endpoint of Quickwit.")
    parser.add_argument("--num-conversations", type=int, default=200, help="Number of agent invocations to simulate.")
    parser.add_argument(
        "--time-range-secs",
        type=int,
        default=3600,
        help="Conversations are spread uniformly over this many seconds before now.",
    )
    parser.add_argument(
        "--no-content",
        action="store_true",
        help="Do not record prompts, completions and tool arguments (they are sensitive in real applications).",
    )
    args = parser.parse_args()

    resource = Resource.create({"service.name": "genai-travel-agent", "service.version": "1.0.0"})
    tracer_provider = TracerProvider(resource=resource)
    tracer_provider.add_span_processor(BatchSpanProcessor(OTLPSpanExporter(endpoint=args.endpoint, insecure=True)))
    tracer = tracer_provider.get_tracer("genai-travel-agent")

    now_secs = time.time()
    for _ in range(args.num_conversations):
        start_secs = now_secs - random.uniform(0, args.time_range_secs)
        run_conversation(tracer, start_secs, capture_content=not args.no_content)

    # Flushes the pending spans before exiting.
    tracer_provider.shutdown()
    print(f"Sent {args.num_conversations} conversations to {args.endpoint}.")


if __name__ == "__main__":
    main()
