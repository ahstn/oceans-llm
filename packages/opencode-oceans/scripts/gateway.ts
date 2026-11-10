import assert from 'node:assert/strict'
import { record, type Entry } from '../src/config'

export const png =
  'iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGP4z8AARAwQCgAf7gP9i18U1AAAAABJRU5ErkJggg=='
export const modelIDs = [
  'claude-fixture',
  'gpt-fixture',
  'chat-fixture',
  'billed-fixture',
  'free-fixture',
]
const formats = [
  'anthropic-messages',
  'openai-responses',
  'openai-chat-completions',
  'openai-chat-completions',
  'openai-chat-completions',
]
const providers = ['anthropic', 'openai', 'generic', 'generic', 'generic']
const sse = (frames: readonly (Entry & { type?: string })[]) =>
  new Response(
    frames
      .map(
        (frame) => `event: ${String(frame.type ?? 'message')}\ndata: ${JSON.stringify(frame)}\n\n`,
      )
      .join('') + 'data: [DONE]\n\n',
    { headers: { 'content-type': 'text/event-stream' } },
  )
export interface CapturedRequest {
  path: string
  body: Entry
}

const groups = {
  data: modelIDs.map((id, index) => ({
    model_group: id,
    providers: [providers[index]],
    supports_vision: true,
    supports_function_calling: true,
    supports_reasoning: true,
    supported_openai_params: ['tools', 'reasoning_effort'],
    max_input_tokens: 128_000,
    max_output_tokens: 4096,
    input_cost_per_token: 0.000003,
    output_cost_per_token: 0.000015,
    cache_read_input_token_cost: 0.0000003,
    cache_creation_input_token_cost: 0.00000375,
  })),
}
const listing = {
  data: modelIDs.map((id, index) => ({
    id,
    context_length: 128_000,
    architecture: {
      input_modalities: ['text', 'image', 'pdf'],
      output_modalities: ['text'],
    },
    client_hints: {
      preferred_api_format: formats[index],
      api_formats: [formats[index]],
      harnesses: {
        opencode: {
          variants: [
            {
              id: 'high',
              settings: index === 0 ? { effort: 'high' } : { reasoningEffort: 'high' },
            },
          ],
        },
      },
    },
    capabilities:
      index === 0
        ? {
            image_input: { supported: true },
            pdf_input: { supported: true },
            effort: { supported: true, high: { supported: true } },
            thinking: { supported: true, types: { adaptive: { supported: true } } },
          }
        : undefined,
  })),
}

function messages(id: string) {
  return sse([
    {
      type: 'message_start',
      message: {
        id: 'msg_fixture',
        type: 'message',
        role: 'assistant',
        model: id,
        content: [],
        stop_reason: null,
        stop_sequence: null,
        usage: {
          input_tokens: 70,
          output_tokens: 0,
          cache_read_input_tokens: 30,
          cache_creation_input_tokens: 10,
        },
      },
    },
    { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } },
    {
      type: 'content_block_delta',
      index: 0,
      delta: { type: 'text_delta', text: 'Fixture reply' },
    },
    { type: 'content_block_stop', index: 0 },
    {
      type: 'message_delta',
      delta: { stop_reason: 'end_turn', stop_sequence: null },
      usage: { output_tokens: 20 },
    },
    { type: 'message_stop' },
  ])
}

function responses(id: string) {
  const response = {
    id: 'resp_fixture',
    object: 'response',
    created_at: 1,
    status: 'completed',
    model: id,
    output: [
      {
        id: 'msg_fixture',
        type: 'message',
        role: 'assistant',
        status: 'completed',
        content: [{ type: 'output_text', text: 'Fixture reply', annotations: [] }],
      },
    ],
    usage: {
      input_tokens: 100,
      output_tokens: 20,
      total_tokens: 120,
      input_tokens_details: { cached_tokens: 30 },
      output_tokens_details: { reasoning_tokens: 5 },
    },
  }
  return sse([
    {
      type: 'response.created',
      response: { ...response, status: 'in_progress', output: [], usage: null },
    },
    {
      type: 'response.output_item.added',
      output_index: 0,
      item: {
        id: 'msg_fixture',
        type: 'message',
        role: 'assistant',
        status: 'in_progress',
        content: [],
      },
    },
    {
      type: 'response.content_part.added',
      item_id: 'msg_fixture',
      output_index: 0,
      content_index: 0,
      part: { type: 'output_text', text: '', annotations: [] },
    },
    {
      type: 'response.output_text.delta',
      item_id: 'msg_fixture',
      output_index: 0,
      content_index: 0,
      delta: 'Fixture reply',
    },
    {
      type: 'response.output_text.done',
      item_id: 'msg_fixture',
      output_index: 0,
      content_index: 0,
      text: 'Fixture reply',
    },
    { type: 'response.output_item.done', output_index: 0, item: response.output[0] },
    { type: 'response.completed', response },
  ])
}

function chat(id: string) {
  return sse([
    {
      id: 'chat_fixture',
      object: 'chat.completion.chunk',
      created: 1,
      model: id,
      choices: [
        {
          index: 0,
          delta: { role: 'assistant', content: 'Fixture reply' },
          finish_reason: null,
        },
      ],
    },
    {
      id: 'chat_fixture',
      object: 'chat.completion.chunk',
      created: 1,
      model: id,
      choices: [{ index: 0, delta: {}, finish_reason: 'stop' }],
    },
    {
      id: 'chat_fixture',
      object: 'chat.completion.chunk',
      created: 1,
      model: id,
      choices: [],
      usage: {
        prompt_tokens: 100,
        completion_tokens: 20,
        total_tokens: 120,
        prompt_tokens_details: { cached_tokens: 30 },
        completion_tokens_details: { reasoning_tokens: 5 },
        ...(id === 'billed-fixture' ? { cost: 0.123 } : id === 'free-fixture' ? { cost: 0 } : {}),
      },
    },
  ])
}

/** Local HTTP boundary only: OpenCode and its actual protocol adapters remain unmodified. */
export function gateway() {
  const requests: CapturedRequest[] = []
  const server = Bun.serve({
    port: 0,
    async fetch(request) {
      const pathname = new URL(request.url).pathname
      assert.equal(
        request.headers.get('authorization') ?? request.headers.get('x-api-key'),
        request.headers.has('x-api-key') ? 'synthetic-key' : 'Bearer synthetic-key',
      )
      if (pathname === '/model_group/info') return Response.json(groups)
      if (pathname === '/v1/models') return Response.json(listing)
      if (request.method !== 'POST') return new Response('missing', { status: 404 })
      const body = record(await request.json())
      requests.push({ path: pathname, body })
      const id = String(body.model)
      if (pathname === '/v1/messages') return messages(id)
      if (pathname === '/v1/responses') return responses(id)
      assert.equal(pathname, '/v1/chat/completions')
      return chat(id)
    },
  })
  return { server, requests }
}
