# Template Transform — `dev.mcpg.transform.template`

> class `transform` · `native` · package `mcpg-plugin-transform-template` · artifact `libmcpg_plugin_transform_template.so` · Apache-2.0

Transform plugin that reshapes a JSON value by rendering an operator-supplied
MiniJinja template — Jinja2 syntax, with `{{ expressions }}`, `{% for %}` and
`{% if %}` blocks, and filters — using the value as the template context. With
`output: string` the rendered text becomes a JSON string, which is the fastest
way to turn a machine-shaped tool result into prose an LLM or a human can read.
With `output: json` the rendered text is parsed back into a structured value, so
the template doubles as a JSON-to-JSON reshaper. Rendering is pure compute — no
I/O, no host calls, no network. Reach for it when the target shape is easier to
write out as a literal with holes in it than to express as a query.

## What it does
- Renders the template with the input value as the root scope, so top-level fields are addressed directly (`{{ name }}`, `{{ user.email }}`).
- Supports the full MiniJinja statement set — loops, conditionals, `loop.last`, and filters such as `|upper` and `|length`.
- Emits either the rendered text as a JSON string or, with `output: json`, the value that text parses to.
- Renders a sub-value when an RFC 6901 JSON Pointer is given, replacing just that sub-value and preserving the surrounding payload.
- Rejects renders larger than `max_output_bytes`, so a runaway loop cannot exhaust gateway memory.
- Compiles a fresh environment per call, which keeps the transform stateless and free of cross-request template caching.
- Declares no `required_capabilities` — it never calls back into the host for network, filesystem, or secret access.

## Configuration
Loaded from the flat top-level `plugins:` list. An entry there joins the global
transform chain and sees every tool call; the same registered plugin can also be
named by a pipeline `plugin_transform` step for a single binding.

```yaml
plugins:
  - id: dev.mcpg.transform.template
    class: transform
    source: { oci: ghcr.io/mcpg-dev/source-code/plugins/transform-template:protocol-1 }
    config:
      phase: result
      pointer: /structuredContent
      output: json
      template: '{"summary": "Order {{ id }}: {{ items|length }} item(s), total {{ total }}"}'
```

In the global chain the pre-dispatch value is the tool's `arguments` object and
the post-dispatch value is the serialised tool result — `content`, optional
`structuredContent`, `isError` — so a `phase: result` template renders against
that envelope unless a pointer narrows it to the payload first.

| Field | Type | Default | Description |
|---|---|---|---|
| `template` | string | *(required)* | The MiniJinja template, rendered with the input value as context. |
| `output` | `string` \| `json` | `string` | Wrap the rendered text as a JSON string, or parse it back into a structured value. |
| `pointer` | string (RFC 6901) | whole value | Render using the sub-value at this pointer and replace it; the rest of the payload is preserved. |
| `phase` | `arguments` \| `result` \| `both` | `both` | Which dispatch phase the global chain fires this transform on. A pipeline step always dispatches through the result path, so `arguments` there turns the step into a no-op. |
| `max_output_bytes` | integer | `1048576` | Reject renders whose output exceeds this size. |

Unknown fields are rejected.

With `output: json` the template must emit valid JSON; anything else is a
transform error rather than a string fallback:

```yaml
plugins:
  - id: dev.mcpg.transform.template
    class: transform
    source: { path: ./plugins/libmcpg_plugin_transform_template.so }
    config:
      phase: arguments
      output: json
      template: '{"who": "{{ user.name }}", "n": {{ user.count }}}'
```

Referenced from a pipeline instead, the plugin receives the whole pipeline
context — `arguments`, `tool_name`, `steps`, and `context` — as its render
context, so a template reads a prior step through `steps.<id>.output` or a
pointer narrows the context first:

```yaml
mcp:
  capabilities:
    tools:
      - name: orders.summary
        description: Fetch an order and return a human-readable summary.
        backend:
          kind: pipeline
          steps:
            - kind: http
              id: fetch
              url: https://orders.example.com/one
            - kind: plugin_transform
              id: summarise
              plugin: dev.mcpg.transform.template
              config:
                pointer: /steps/fetch/output
                template: "Order {{ id }} for {{ customer }} — {{ status }}"
```

## Security
The template comes from the entry's `config:`, which is operator-authored and
config-origin. Request data supplies only the render context, never the template
source, so a caller cannot inject template syntax that the engine will execute.
MiniJinja's default undefined behaviour is lenient: a field the context does not
contain renders as empty rather than raising. That keeps a partial payload from
failing the whole render, but it also means a typo in a field name yields a
silently empty slot — check rendered output when you first wire a template up.

A template parse error, a `pointer` that does not resolve, an unknown config
key, and an over-budget render are all transform errors. In the global chain an
error is logged as a warning and the last good value is carried forward; in a
pipeline `plugin_transform` step the same error fails the step.

## Observability
Every application through the global chain increments
`mcpg_transform_applies_total` (labels `plugin_id`, `phase` of `pre` or `post`,
`outcome` of `unchanged`, `modified`, or `error`) and records
`mcpg_transform_apply_ms`. A modification also emits the
`mcpg.transform.applied` audit event, which carries hashes and byte counts of
the before and after values rather than their plaintext.

## Build
Default feature set is OFF (avoids `mcpg_plugin_register` linker
collisions in the workspace build); opt in to the cdylib:

```bash
cargo build -p mcpg-plugin-transform-template --features cdylib-export --release   # → target/release/libmcpg_plugin_transform_template.so
```

## Sign & load (production)
Sign the artifact, pin/verify via the entry's `signature:` block, and honour
revocations. See <https://mcpg.dev/docs/security/plugin-security>.

## See also
- Pipeline step reference: <https://mcpg.dev/docs/reference/pipeline-steps>
- What a plugin is and how the ABI works: <https://mcpg.dev/docs/plugins/plugins-and-protocol>
- Query-style reshaping instead of a literal template: `libs/plugins/transform/jsonata`
- Format conversion and validation: `libs/plugins/transform/csv`, `libs/plugins/transform/json-schema`
