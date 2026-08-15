//! Template-engine transform plugin.
//!
//! Renders an operator-supplied MiniJinja template with the input JSON value as
//! the template context. Stateless apart from the manifest — the template +
//! options arrive per call in `config`, so one instance serves both the global
//! transform chain (pre/post dispatch) and the pipeline `plugin_transform`
//! bridge. Pure compute; no host calls.
//!
//! - `output: string` (default) → the rendered text as a JSON string.
//! - `output: json` → the rendered text parsed back into a structured value
//!   (the template is expected to emit valid JSON).
//!
//! An optional JSON Pointer (`pointer`) selects a sub-value to use as the
//! render context and to replace; the surrounding payload is preserved.

use mcpg_plugin_protocol::{PluginContext, PluginManifest, TransformResult, firstparty_manifest};
use mcpg_plugin_sdk::ffi::SyncTransform;
use serde::Deserialize;
use serde_json::Value;

const DEFAULT_MAX_OUTPUT_BYTES: usize = 1_048_576;
const TEMPLATE_NAME: &str = "t";

/// Which dispatch phase(s) a global transform fires on. Ignored by the
/// pipeline bridge (the host calls `transform_result` directly there).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Arguments,
    Result,
    #[default]
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Output {
    /// Rendered text as a JSON string.
    #[default]
    String,
    /// Rendered text parsed back into a structured JSON value.
    Json,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateConfig {
    /// The MiniJinja template, rendered with the input value as context.
    template: String,
    #[serde(default)]
    output: Output,
    /// JSON Pointer (RFC 6901) to the sub-value to use as context + replace.
    /// When omitted (or `""`), the whole value is used.
    #[serde(default)]
    pointer: Option<String>,
    #[serde(default)]
    phase: Phase,
    #[serde(default = "default_max_output_bytes")]
    max_output_bytes: usize,
}

fn default_max_output_bytes() -> usize {
    DEFAULT_MAX_OUTPUT_BYTES
}

pub struct TemplateTransform {
    manifest: PluginManifest,
}

impl TemplateTransform {
    pub fn new(_config_json: &str) -> Self {
        Self {
            manifest: firstparty_manifest! {
                id: "dev.mcpg.transform.template",
                name: "Template Transform",
                class: Transform,
            },
        }
    }

    fn run(&self, value: &Value, config: &Value, phase: Phase) -> TransformResult {
        let cfg: TemplateConfig = match serde_json::from_value(config.clone()) {
            Ok(c) => c,
            Err(e) => {
                return TransformResult::Error {
                    message: format!("template transform config: {e}"),
                };
            }
        };
        // Global-mode phase gating; pipeline-mode always calls transform_result.
        if cfg.phase != Phase::Both && cfg.phase != phase {
            return TransformResult::Unchanged;
        }

        let ptr = cfg.pointer.as_deref().unwrap_or("");
        let target = match value.pointer(ptr) {
            Some(t) => t,
            None => {
                return TransformResult::Error {
                    message: format!("pointer {ptr:?} not found in value"),
                };
            }
        };

        let rendered = match render(&cfg.template, target) {
            Ok(s) => s,
            Err(message) => return TransformResult::Error { message },
        };
        if rendered.len() > cfg.max_output_bytes {
            return TransformResult::Error {
                message: format!(
                    "template output {} bytes exceeds max_output_bytes ({})",
                    rendered.len(),
                    cfg.max_output_bytes
                ),
            };
        }

        let produced = match cfg.output {
            Output::String => Value::String(rendered),
            Output::Json => match serde_json::from_str(&rendered) {
                Ok(v) => v,
                Err(e) => {
                    return TransformResult::Error {
                        message: format!("output: json — rendered text is not valid JSON: {e}"),
                    };
                }
            },
        };

        if ptr.is_empty() {
            TransformResult::Modified { value: produced }
        } else {
            let mut out = value.clone();
            match out.pointer_mut(ptr) {
                Some(slot) => {
                    *slot = produced;
                    TransformResult::Modified { value: out }
                }
                None => TransformResult::Error {
                    message: format!("pointer {ptr:?} not assignable"),
                },
            }
        }
    }
}

impl SyncTransform for TemplateTransform {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn transform_arguments(
        &self,
        _ctx: &PluginContext,
        arguments: &Value,
        config: &Value,
    ) -> TransformResult {
        self.run(arguments, config, Phase::Arguments)
    }

    fn transform_result(
        &self,
        _ctx: &PluginContext,
        result: &Value,
        config: &Value,
    ) -> TransformResult {
        self.run(result, config, Phase::Result)
    }
}

/// Render `template` with `context` (the input value) as the root scope. A
/// fresh `Environment` per call keeps the transform stateless; templates are
/// small, so compile-per-call is acceptable (mirrors the jsonata parse-per-call
/// posture).
fn render(template: &str, context: &Value) -> Result<String, String> {
    let mut env = minijinja::Environment::new();
    env.add_template(TEMPLATE_NAME, template)
        .map_err(|e| format!("template parse: {e}"))?;
    let tmpl = env
        .get_template(TEMPLATE_NAME)
        .map_err(|e| format!("template load: {e}"))?;
    tmpl.render(context)
        .map_err(|e| format!("template render: {e}"))
}

// cdylib export — gated so a plain workspace build emits only the rlib (no
// duplicate `mcpg_plugin_register` symbol across plugin crates).
#[cfg(any(feature = "cdylib-export", feature = "static-firstparty"))]
mcpg_plugin_sdk::declare_plugin! {
    plugin_id: "dev.mcpg.transform.template",
    plugin_version: env!("CARGO_PKG_VERSION"),
    descriptor_yaml: include_str!("../plugin.yaml"),
    capabilities: &[],
    entities: [
        transform as xform {
            inner_name: "",
            plugin_type: TemplateTransform,
            factory: |cfg: &str, _host: ::mcpg_plugin_sdk::HostHandle| TemplateTransform::new(cfg),
        },
    ],
}

#[cfg(test)]
mod tests;
