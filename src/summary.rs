use anyhow::{anyhow, Result};

use crate::config::SummaryConfig;

#[derive(Debug)]
pub enum ParsedSummary {
    Normal(String),
    Degraded(String),
}

impl ParsedSummary {
    pub fn into_markdown(self) -> String {
        match self {
            Self::Normal(s) | Self::Degraded(s) => s,
        }
    }
    pub fn is_degraded(&self) -> bool {
        matches!(self, Self::Degraded(_))
    }
}

pub fn call_summary_api(
    config: &SummaryConfig,
    transcript: &str,
    participants: &str,
) -> Result<ParsedSummary> {
    let prompt = config
        .prompt_template
        .replace("{transcript}", transcript)
        .replace("{participants}", participants);

    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": config.model,
        "messages": [{"role": "user", "content": prompt}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "save_summary",
                "description": "Save the meeting summary as markdown",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "markdown": {
                            "type": "string",
                            "description": "The meeting summary in Markdown format"
                        }
                    },
                    "required": ["markdown"]
                }
            }
        }],
        "tool_choice": {"type": "function", "function": {"name": "save_summary"}}
    });

    let mut response = ureq::post(&url)
        .header("Authorization", &format!("Bearer {}", config.api_key))
        .send_json(&body)
        .map_err(|e| anyhow!("HTTP: {e}"))?;

    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| anyhow!("reading response: {e}"))?;

    let preview: String = text.chars().take(500).collect();
    log::debug!(
        "summary API response ({}B): {}{}",
        text.len(),
        preview,
        if text.len() > preview.len() { "…" } else { "" }
    );

    parse_summary_response(&text)
}

pub(crate) fn parse_summary_response(text: &str) -> Result<ParsedSummary> {
    let json: serde_json::Value =
        serde_json::from_str(text).map_err(|e| anyhow!("parsing JSON: {e}"))?;

    // Surface API-level errors with their own message instead of a generic hint
    if let Some(err_msg) = json["error"]["message"].as_str() {
        return Err(anyhow!("{err_msg}"));
    }

    // Path 1: standard tool_calls
    if let Some(tc) = json["choices"][0]["message"]["tool_calls"].as_array() {
        if !tc.is_empty() {
            let args_str = tc[0]["function"]["arguments"]
                .as_str()
                .ok_or_else(|| anyhow!("Réponse invalide : arguments du tool call manquants"))?;
            let args: serde_json::Value = serde_json::from_str(args_str)
                .map_err(|e| anyhow!("Parsing des arguments : {e}"))?;
            let md = args["markdown"]
                .as_str()
                .ok_or_else(|| anyhow!("Champ 'markdown' manquant dans les arguments du tool call"))?;
            return Ok(ParsedSummary::Normal(md.to_string()));
        }
    }

    // Path 2: legacy OpenAI function_call format
    if let Some(args_str) =
        json["choices"][0]["message"]["function_call"]["arguments"].as_str()
    {
        if let Ok(args) = serde_json::from_str::<serde_json::Value>(args_str) {
            if let Some(md) = args["markdown"].as_str() {
                return Ok(ParsedSummary::Normal(md.to_string()));
            }
        }
    }

    // Paths 3 & 4: content-based fallbacks (model ignored tool_choice)
    if let Some(content) = json["choices"][0]["message"]["content"].as_str() {
        // Path 3: content is a JSON object {"markdown": "..."}
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
            if let Some(md) = parsed["markdown"].as_str() {
                log::info!("summary: using content-JSON fallback (no tool_calls)");
                return Ok(ParsedSummary::Degraded(md.to_string()));
            }
        }

        // Path 4: content is raw markdown (last resort)
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            log::warn!("summary: model did not honour tool_choice — saving raw content as markdown");
            return Ok(ParsedSummary::Degraded(trimmed.to_string()));
        }
    }

    Err(anyhow!(
        "Le modèle n'a pas utilisé le tool_choice requis. \
        Essayez un autre modèle dans les paramètres (⚙)."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool_calls_response(markdown: &str) -> String {
        let args = serde_json::json!({"markdown": markdown}).to_string();
        serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "save_summary",
                            "arguments": args
                        }
                    }]
                }
            }]
        })
        .to_string()
    }

    fn make_valid_response(markdown: &str) -> String {
        make_tool_calls_response(markdown)
    }

    // ─── existing tests (updated for new return type) ───────────────────────

    #[test]
    fn parse_valid_response_returns_markdown() {
        let markdown = "# Summary\n\nKey points:\n- point 1";
        let response = make_valid_response(markdown);
        let result = parse_summary_response(&response).unwrap();
        assert!(!result.is_degraded());
        assert_eq!(result.into_markdown(), markdown);
    }

    #[test]
    fn parse_response_empty_tool_calls_returns_error() {
        let response = r#"{"choices":[{"message":{"tool_calls":[]}}]}"#;
        let err = parse_summary_response(response).unwrap_err();
        assert!(err.to_string().contains("tool_choice"), "{}", err);
    }

    #[test]
    fn parse_response_missing_tool_calls_uses_content_fallback() {
        // Previously returned an error; now falls through to raw-content path 4
        let md = "# Meeting\n\nSummary text";
        let response =
            serde_json::json!({"choices": [{"message": {"content": md}}]}).to_string();
        let result = parse_summary_response(&response).unwrap();
        assert!(result.is_degraded());
        assert_eq!(result.into_markdown(), md);
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        let err = parse_summary_response("not json").unwrap_err();
        assert!(err.to_string().contains("parsing JSON"), "{}", err);
    }

    #[test]
    fn parse_missing_markdown_field_returns_error() {
        let args = serde_json::json!({"other_field": "value"}).to_string();
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "save_summary",
                            "arguments": args
                        }
                    }]
                }
            }]
        })
        .to_string();
        let err = parse_summary_response(&response).unwrap_err();
        assert!(err.to_string().contains("markdown"), "{}", err);
    }

    #[test]
    fn parse_response_with_unicode_markdown() {
        let markdown = "# Réunion\n\nPoints clés :\n- Décision prise ✓";
        let response = make_valid_response(markdown);
        assert_eq!(
            parse_summary_response(&response).unwrap().into_markdown(),
            markdown
        );
    }

    #[test]
    fn call_summary_api_prompt_substitution() {
        let config = crate::config::SummaryConfig {
            base_url: "https://example.com".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-4o-mini".to_string(),
            prompt_template: "Transcript: {transcript}\nParticipants: {participants}".to_string(),
        };
        let prompt = config
            .prompt_template
            .replace("{transcript}", "Hello world")
            .replace("{participants}", "Alice, Bob");
        assert_eq!(prompt, "Transcript: Hello world\nParticipants: Alice, Bob");
    }

    // ─── 4 fallback-path tests (one per parsing path) ───────────────────────

    #[test]
    fn parse_path1_tool_calls_returns_normal() {
        let md = "# Meeting\n\nSummary";
        let response = make_tool_calls_response(md);
        let result = parse_summary_response(&response).unwrap();
        assert!(!result.is_degraded());
        assert_eq!(result.into_markdown(), md);
    }

    #[test]
    fn parse_path2_function_call_legacy_returns_normal() {
        let md = "# Legacy Summary";
        let args = serde_json::json!({"markdown": md}).to_string();
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "function_call": {
                        "name": "save_summary",
                        "arguments": args
                    }
                }
            }]
        })
        .to_string();
        let result = parse_summary_response(&response).unwrap();
        assert!(!result.is_degraded());
        assert_eq!(result.into_markdown(), md);
    }

    #[test]
    fn parse_path3_content_json_returns_degraded() {
        let md = "# JSON Content Summary";
        let content = serde_json::json!({"markdown": md}).to_string();
        let response = serde_json::json!({
            "choices": [{
                "message": { "content": content }
            }]
        })
        .to_string();
        let result = parse_summary_response(&response).unwrap();
        assert!(result.is_degraded());
        assert_eq!(result.into_markdown(), md);
    }

    #[test]
    fn parse_path4_raw_content_returns_degraded() {
        let md = "# Raw Markdown\n\nSome content without tool call";
        let response = serde_json::json!({
            "choices": [{
                "message": { "content": md }
            }]
        })
        .to_string();
        let result = parse_summary_response(&response).unwrap();
        assert!(result.is_degraded());
        assert_eq!(result.into_markdown(), md);
    }

    #[test]
    fn parse_api_error_returns_error_message() {
        let response =
            r#"{"error":{"message":"Rate limit exceeded","type":"rate_limit_error"}}"#;
        let err = parse_summary_response(response).unwrap_err();
        assert!(err.to_string().contains("Rate limit exceeded"), "{}", err);
    }
}
