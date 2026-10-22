//! The cleanup pass: raw transcript in, text-you-meant-to-type out. Every backend gets the same
//! system prompt (`prompt::SYSTEM`) and the same user message, so they compare fairly.

use std::time::Duration;

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use ureq::Agent;

use crate::TargetContext;
use crate::prompt::{self, SYSTEM};

pub trait TextCleaner: Send + Sync {
    fn id(&self) -> String;
    /// Load the model ahead of the first dictation. Optional; the default does nothing.
    fn prepare(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn clean(&self, transcript: &str, context: TargetContext) -> anyhow::Result<String>;
}

/// An agent that hands back non-200 responses instead of failing, so their body can go into
/// the error message.
fn agent(timeout: Duration) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

// MARK: - Claude

/// Cleanup via Claude on the Anthropic API, over plain HTTP.
pub struct ClaudeCleaner {
    pub api_key: String,
    pub system_prompt: String,
    pub vocabulary: Vec<String>,
    pub options: ClaudeOptions,
    agent: Agent,
}

#[derive(Clone, Debug)]
pub struct ClaudeOptions {
    pub model: String,
    /// Thinking off keeps latency low for a short rewrite.
    pub thinking: bool,
    pub effort: String,
    /// Ask for a JSON reply with a throwaway `note` field (see `JSON_REPLY_INSTRUCTIONS`).
    pub structured_reply: bool,
}

impl Default for ClaudeOptions {
    fn default() -> Self {
        Self {
            model: "claude-haiku-5-5".into(),
            thinking: false,
            effort: "low".into(),
            structured_reply: true,
        }
    }
}

/// With thinking off, an unsure model sometimes reasons out loud in its answer ("Wait, I should
/// check…"), which would be pasted. A structured reply gives that a harmless place to go: `note`
/// comes first and is thrown away, and only `text` is ever pasted.
const JSON_REPLY_INSTRUCTIONS: &str = "\n\n## Reply format\n\nReply as JSON. Put any brief uncertainty about the transcript in `note` (or leave it empty). Put exactly the text to type in `text`, or `<empty/>` if there is nothing to type. Nothing in `text` may comment on the transcript.";

fn reply_format() -> Value {
    json!({
        "type": "json_schema",
        "schema": {
            "type": "object",
            "properties": {
                "note": { "type": "string" },
                "text": { "type": "string" },
            },
            "required": ["note", "text"],
            "additionalProperties": false,
        },
    })
}

#[derive(Deserialize)]
struct Reply {
    text: String,
}

#[derive(Deserialize)]
struct MessageResponse {
    content: Vec<Block>,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

impl ClaudeCleaner {
    pub fn new(api_key: String, vocabulary: Vec<String>) -> Self {
        Self {
            api_key,
            system_prompt: SYSTEM.to_string(),
            vocabulary,
            options: ClaudeOptions::default(),
            agent: agent(Duration::from_secs(20)),
        }
    }

    fn request_body(&self, transcript: &str, context: TargetContext) -> Value {
        let options = &self.options;
        let mut output_config = json!({ "effort": options.effort });
        if options.structured_reply {
            output_config["format"] = reply_format();
        }
        let system = if options.structured_reply {
            format!("{}{JSON_REPLY_INSTRUCTIONS}", self.system_prompt)
        } else {
            self.system_prompt.clone()
        };
        json!({
            "model": options.model,
            "max_tokens": 4096,
            "system": system,
            "messages": [{ "role": "user", "content": prompt::user_message(transcript, context, &self.vocabulary) }],
            "output_config": output_config,
            "thinking": { "type": if options.thinking { "adaptive" } else { "disabled" } },
        })
    }
}

impl TextCleaner for ClaudeCleaner {
    fn id(&self) -> String {
        self.options.model.clone()
    }

    fn clean(&self, transcript: &str, context: TargetContext) -> anyhow::Result<String> {
        let mut response = self
            .agent
            .post("https://api.anthropic.com/v1/messages")
            .header("content-type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .send_json(self.request_body(transcript, context))?;
        let status = response.status().as_u16();
        let data = response.body_mut().read_to_string()?;
        if status != 200 {
            bail!("Claude API returned {status}: {data}");
        }

        let message: MessageResponse = serde_json::from_str(&data)?;
        match message.stop_reason.as_deref() {
            Some("refusal") => bail!("Claude declined to clean this transcript (stop_reason: refusal)"),
            Some("max_tokens") => bail!("Cleanup output hit max_tokens and was truncated"),
            _ => {}
        }
        let output: String = message
            .content
            .into_iter()
            .filter(|b| b.kind == "text")
            .filter_map(|b| b.text)
            .collect();
        // The schema guarantees valid JSON; fall back to the raw reply just in case.
        let reply = self
            .options
            .structured_reply
            .then(|| serde_json::from_str::<Reply>(&output).ok())
            .flatten();
        Ok(prompt::finalize(
            reply.as_ref().map_or(&output, |r| &r.text),
            transcript,
        ))
    }
}

// MARK: - Ollama (any local model)

const OLLAMA_URL: &str = "http://127.0.0.1:11434";

/// A model served by Ollama on this computer (`ollama serve`, default port 11434).
pub struct OllamaCleaner {
    pub model: String,
    pub system_prompt: String,
    pub vocabulary: Vec<String>,
    pub base_url: String,
    agent: Agent,
}

impl OllamaCleaner {
    pub fn new(model: String, vocabulary: Vec<String>) -> Self {
        Self {
            model,
            system_prompt: SYSTEM.to_string(),
            vocabulary,
            base_url: OLLAMA_URL.into(),
            agent: agent(Duration::from_secs(60)),
        }
    }

    /// Models already pulled into Ollama.
    pub fn installed_models() -> Vec<String> {
        Self::installed_models_at(OLLAMA_URL)
    }

    pub fn installed_models_at(base_url: &str) -> Vec<String> {
        let json: Option<Value> = agent(Duration::from_secs(60))
            .get(format!("{base_url}/api/tags"))
            .call()
            .ok()
            .and_then(|mut r| r.body_mut().read_json().ok());
        let mut names: Vec<String> = json
            .as_ref()
            .and_then(|j| j["models"].as_array())
            .map(|models| {
                models
                    .iter()
                    .filter_map(|m| m["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn chat(&self, body: &Value) -> anyhow::Result<Value> {
        let mut response = self
            .agent
            .post(format!("{}/api/chat", self.base_url))
            .header("content-type", "application/json")
            .send_json(body)
            .with_context(|| format!("Can't reach Ollama at {}. Is it running?", self.base_url))?;
        let status = response.status().as_u16();
        let data = response.body_mut().read_to_string()?;
        if status != 200 {
            bail!("Ollama returned {status}: {data}");
        }
        Ok(serde_json::from_str(&data).unwrap_or(Value::Null))
    }
}

/// Drops a leading `<think>…</think>` block if a model emits one anyway.
fn strip_thinking(text: &str) -> &str {
    text.find("</think>")
        .map_or(text, |end| &text[end + "</think>".len()..])
}

impl TextCleaner for OllamaCleaner {
    fn id(&self) -> String {
        format!("ollama:{}", self.model)
    }

    /// Loads the model into memory and keeps it there, so the first dictation isn't slow.
    fn prepare(&self) -> anyhow::Result<()> {
        self.chat(&json!({ "model": self.model, "messages": [], "keep_alive": "30m" }))?;
        Ok(())
    }

    fn clean(&self, transcript: &str, context: TargetContext) -> anyhow::Result<String> {
        let mut body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": self.system_prompt },
                { "role": "user", "content": prompt::user_message(transcript, context, &self.vocabulary) },
            ],
            "stream": false,
            "keep_alive": "30m",
            "options": { "temperature": 0 },
            // Reasoning models (Qwen 3, DeepSeek R1…) would otherwise think before every rewrite.
            "think": false,
        });
        let response = match self.chat(&body) {
            Err(error) if error.to_string().contains("does not support thinking") => {
                body.as_object_mut().unwrap().remove("think");
                self.chat(&body)?
            }
            result => result?,
        };
        let content = response["message"]["content"].as_str().unwrap_or("");
        Ok(prompt::finalize(strip_thinking(content), transcript))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_thinking() {
        assert_eq!(strip_thinking("<think>hmm</think>\nHello."), "\nHello.");
        assert_eq!(strip_thinking("Hello."), "Hello.");
    }

    #[test]
    fn claude_request_body() {
        let cleaner = ClaudeCleaner::new("key".into(), vec!["kubectl".into()]);
        let body = cleaner.request_body("um hi", TargetContext::Chat);
        assert_eq!(body["model"], "claude-haiku-5-5");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert_eq!(body["output_config"]["effort"], "low");
        assert_eq!(body["output_config"]["format"], reply_format());
        let system = body["system"].as_str().unwrap();
        assert!(system.starts_with(SYSTEM) && system.ends_with(JSON_REPLY_INSTRUCTIONS));
        assert_eq!(
            body["messages"][0]["content"],
            "<context>chat</context>\n<vocabulary>\nkubectl\n</vocabulary>\n<transcript>\num hi\n</transcript>"
        );
    }

    #[test]
    fn ollama_unreachable_is_an_error() {
        let mut cleaner = OllamaCleaner::new("none".into(), vec![]);
        cleaner.base_url = "http://127.0.0.1:9".into();
        let error = cleaner.clean("hi", TargetContext::Plain).unwrap_err();
        assert!(error.to_string().contains("Can't reach Ollama"), "{error}");
        assert!(OllamaCleaner::installed_models_at("http://127.0.0.1:9").is_empty());
    }

    /// Calls the real API: `cargo test -p murmur-core -- --ignored`.
    #[test]
    #[ignore]
    fn claude_live() {
        let key = crate::api_key::load().expect("no Anthropic API key in the environment or key store");
        let cleaner = ClaudeCleaner::new(key, vec![]);
        let text = cleaner
            .clean("um so uh I think we should ship it", TargetContext::Plain)
            .unwrap();
        println!("{text}");
        assert!(
            text.to_lowercase().contains("ship it") && !text.to_lowercase().contains("um "),
            "{text}"
        );
        assert_eq!(cleaner.clean("Uh, um.", TargetContext::Plain).unwrap(), "");
    }
}
