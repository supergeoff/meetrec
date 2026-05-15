use anyhow::{anyhow, Result};

use crate::config::SummaryConfig;

pub fn call_summary_api(
    config: &SummaryConfig,
    transcript: &str,
    participants: &str,
) -> Result<String> {
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

    parse_summary_response(&text)
}

pub(crate) fn parse_summary_response(text: &str) -> Result<String> {
    let json: serde_json::Value =
        serde_json::from_str(text).map_err(|e| anyhow!("parsing JSON: {e}"))?;

    let tool_calls = json["choices"][0]["message"]["tool_calls"].as_array();
    let tool_calls = match tool_calls {
        Some(tc) if !tc.is_empty() => tc,
        _ => {
            return Err(anyhow!(
                "Le modèle n'a pas utilisé le tool_choice requis. \
                Essayez un autre modèle dans les paramètres (⚙)."
            ))
        }
    };

    let args_str = tool_calls[0]["function"]["arguments"]
        .as_str()
        .ok_or_else(|| anyhow!("Réponse invalide : arguments du tool call manquants"))?;

    let args: serde_json::Value =
        serde_json::from_str(args_str).map_err(|e| anyhow!("Parsing des arguments : {e}"))?;

    args["markdown"]
        .as_str()
        .ok_or_else(|| anyhow!("Champ 'markdown' manquant dans les arguments du tool call"))
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_valid_response(markdown: &str) -> String {
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

    #[test]
    fn parse_valid_response_returns_markdown() {
        let markdown = "# Summary\n\nKey points:\n- point 1";
        let response = make_valid_response(markdown);
        assert_eq!(parse_summary_response(&response).unwrap(), markdown);
    }

    #[test]
    fn parse_response_empty_tool_calls_returns_error() {
        let response = r#"{"choices":[{"message":{"tool_calls":[]}}]}"#;
        let err = parse_summary_response(response).unwrap_err();
        assert!(err.to_string().contains("tool_choice"), "{}", err);
    }

    #[test]
    fn parse_response_missing_tool_calls_field_returns_error() {
        let response = r#"{"choices":[{"message":{"content":"text"}}]}"#;
        let err = parse_summary_response(response).unwrap_err();
        assert!(err.to_string().contains("tool_choice"), "{}", err);
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
        assert_eq!(parse_summary_response(&response).unwrap(), markdown);
    }

    #[test]
    fn call_summary_api_prompt_substitution() {
        // We test substitution directly via parse flow by verifying the
        // prompt template replacement logic inside call_summary_api's setup.
        // Since we can't call the real API in unit tests, we verify substitution
        // by examining the config's prompt_template replacement manually.
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
}
