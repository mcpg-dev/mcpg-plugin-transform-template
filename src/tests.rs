use mcpg_plugin_protocol::{PluginContext, PluginIdentity, TransformResult};
use mcpg_plugin_sdk::ffi::SyncTransform;
use serde_json::json;

use super::TemplateTransform;

fn ctx() -> PluginContext {
    PluginContext {
        request_id: "t".into(),
        session_id: None,
        tool_name: "x".into(),
        surface: "tool".into(),
        identity: PluginIdentity {
            kind: "anonymous".into(),
            trust_level: "unauthenticated".into(),
            subject_id: None,
            auth_provider: None,
            issuer: None,
            roles: Vec::new(),
            groups: Vec::new(),
            scopes: Vec::new(),
            attributes: Default::default(),
        },
        transport: "http".into(),
    }
}

fn modified(r: TransformResult) -> serde_json::Value {
    match r {
        TransformResult::Modified { value } => value,
        other => panic!("expected Modified, got {other:?}"),
    }
}

fn error_msg(r: TransformResult) -> String {
    match r {
        TransformResult::Error { message } => message,
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn renders_string_from_fields() {
    let p = TemplateTransform::new("{}");
    let cfg = json!({ "template": "Hello {{ name }}, you have {{ count }} messages" });
    let input = json!({ "name": "Alice", "count": 3 });
    let out = modified(p.transform_result(&ctx(), &input, &cfg));
    assert_eq!(out, json!("Hello Alice, you have 3 messages"));
}

#[test]
fn renders_with_loops_and_filters() {
    let p = TemplateTransform::new("{}");
    let cfg = json!({
        "template": "{% for i in items %}{{ i.name|upper }}{% if not loop.last %},{% endif %}{% endfor %}"
    });
    let input = json!({ "items": [ { "name": "a" }, { "name": "b" } ] });
    let out = modified(p.transform_result(&ctx(), &input, &cfg));
    assert_eq!(out, json!("A,B"));
}

#[test]
fn output_json_parses_rendered_text_into_value() {
    let p = TemplateTransform::new("{}");
    let cfg = json!({
        "template": "{\"greeting\": \"hi {{ name }}\", \"n\": {{ count }}}",
        "output": "json"
    });
    let input = json!({ "name": "bob", "count": 7 });
    let out = modified(p.transform_result(&ctx(), &input, &cfg));
    assert_eq!(out, json!({ "greeting": "hi bob", "n": 7 }));
}

#[test]
fn output_json_invalid_json_is_error() {
    let p = TemplateTransform::new("{}");
    let cfg = json!({ "template": "not json {{ name }}", "output": "json" });
    let msg = error_msg(p.transform_result(&ctx(), &json!({ "name": "x" }), &cfg));
    assert!(msg.contains("not valid JSON"), "{msg}");
}

#[test]
fn undefined_variable_renders_empty() {
    // MiniJinja's default undefined behaviour is lenient: a missing field
    // renders as empty rather than erroring.
    let p = TemplateTransform::new("{}");
    let cfg = json!({ "template": "[{{ missing }}]" });
    let out = modified(p.transform_result(&ctx(), &json!({}), &cfg));
    assert_eq!(out, json!("[]"));
}

#[test]
fn pointer_renders_subfield_and_preserves_rest() {
    let p = TemplateTransform::new("{}");
    let cfg = json!({ "template": "{{ first }} {{ last }}", "pointer": "/user" });
    let input = json!({ "user": { "first": "Ada", "last": "Lovelace" }, "id": 9 });
    let out = modified(p.transform_result(&ctx(), &input, &cfg));
    assert_eq!(out, json!({ "user": "Ada Lovelace", "id": 9 }));
}

#[test]
fn pointer_not_found_is_error() {
    let p = TemplateTransform::new("{}");
    let cfg = json!({ "template": "{{ x }}", "pointer": "/missing" });
    let msg = error_msg(p.transform_result(&ctx(), &json!({ "a": 1 }), &cfg));
    assert!(msg.contains("not found"), "{msg}");
}

#[test]
fn template_parse_error_is_error() {
    let p = TemplateTransform::new("{}");
    let cfg = json!({ "template": "{% for x in %}" });
    let msg = error_msg(p.transform_result(&ctx(), &json!({}), &cfg));
    assert!(msg.contains("template parse"), "{msg}");
}

#[test]
fn phase_arguments_skips_result_phase() {
    let p = TemplateTransform::new("{}");
    let cfg = json!({ "template": "{{ a }}", "phase": "arguments" });
    assert!(matches!(
        p.transform_result(&ctx(), &json!({ "a": 1 }), &cfg),
        TransformResult::Unchanged
    ));
    assert!(matches!(
        p.transform_arguments(&ctx(), &json!({ "a": 1 }), &cfg),
        TransformResult::Modified { .. }
    ));
}

#[test]
fn rejects_unknown_config_field() {
    let p = TemplateTransform::new("{}");
    let cfg = json!({ "template": "{{ a }}", "bogus": 1 });
    let msg = error_msg(p.transform_result(&ctx(), &json!({ "a": 1 }), &cfg));
    assert!(msg.contains("config"), "{msg}");
}

#[test]
fn enforces_max_output_bytes() {
    let p = TemplateTransform::new("{}");
    let cfg = json!({ "template": "{{ s }}", "max_output_bytes": 3 });
    let msg = error_msg(p.transform_result(&ctx(), &json!({ "s": "way too long" }), &cfg));
    assert!(msg.contains("max_output_bytes"), "{msg}");
}
