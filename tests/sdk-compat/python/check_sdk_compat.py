"""Feeds gateway SSE fixtures to the official Python SDKs with strict response validation.

Run with `mise run //tests/sdk-compat:python`. The SDKs are unmodified; the check fails if
either one rejects the `usage.oceans.cost` field the gateway adds to final usage events.
"""

import pathlib
import sys

import anthropic
import httpx2 as httpx
import openai

FIXTURES = pathlib.Path(__file__).resolve().parent.parent / "fixtures"
COST = "0.0012"


def transport(name: str) -> httpx.Client:
    body = (FIXTURES / f"{name}.sse").read_bytes()

    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            200, headers={"content-type": "text/event-stream; charset=utf-8"}, content=body
        )

    return httpx.Client(transport=httpx.MockTransport(handler))


def openai_client(name: str) -> openai.OpenAI:
    return openai.OpenAI(
        api_key="sdk-compat",
        base_url="http://sdk-compat.test/v1",
        http_client=transport(name),
        max_retries=0,
        _strict_response_validation=True,
    )


def anthropic_client(name: str) -> anthropic.Anthropic:
    return anthropic.Anthropic(
        api_key="sdk-compat",
        base_url="http://sdk-compat.test",
        http_client=transport(name),
        max_retries=0,
        _strict_response_validation=True,
    )


def oceans_cost(usage: object) -> object:
    extra = getattr(usage, "model_extra", None) or {}
    return (extra.get("oceans") or {}).get("cost")


def check_chat_completions() -> None:
    text, usage = "", None
    stream = openai_client("chat_completions").chat.completions.create(
        model="fast",
        messages=[{"role": "user", "content": "hi"}],
        stream=True,
        stream_options={"include_usage": True},
    )
    for chunk in stream:
        if chunk.choices:
            text += chunk.choices[0].delta.content or ""
        usage = chunk.usage or usage
    assert text == "Hello world", text
    assert usage is not None and usage.total_tokens == 7, usage
    assert oceans_cost(usage) == COST, usage


def check_responses() -> None:
    completed = None
    stream = openai_client("responses").responses.create(model="fast", input="hi", stream=True)
    for event in stream:
        if event.type == "response.completed":
            completed = event.response
    assert completed is not None and completed.output_text == "Hello world", completed
    assert completed.usage is not None and completed.usage.total_tokens == 7, completed.usage
    assert oceans_cost(completed.usage) == COST, completed.usage

    with openai_client("responses").responses.stream(model="fast", input="hi") as stream:
        final = stream.get_final_response()
    assert final.output_text == "Hello world", final


def check_messages() -> None:
    text, delta_usage = "", None
    stream = anthropic_client("messages").messages.create(
        model="fast", max_tokens=64, messages=[{"role": "user", "content": "hi"}], stream=True
    )
    for event in stream:
        if event.type == "content_block_delta" and event.delta.type == "text_delta":
            text += event.delta.text
        if event.type == "message_delta":
            delta_usage = event.usage
    assert text == "Hello world", text
    assert delta_usage is not None and delta_usage.output_tokens == 2, delta_usage
    assert oceans_cost(delta_usage) == COST, delta_usage

    with anthropic_client("messages").messages.stream(
        model="fast", max_tokens=64, messages=[{"role": "user", "content": "hi"}]
    ) as stream:
        final = stream.get_final_message()
    assert final.content[0].type == "text" and final.content[0].text == "Hello world", final
    assert final.stop_reason == "end_turn", final


def main() -> int:
    for check in (check_chat_completions, check_responses, check_messages):
        check()
        print(f"ok {check.__name__}")
    print(f"openai {openai.__version__}, anthropic {anthropic.__version__}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
