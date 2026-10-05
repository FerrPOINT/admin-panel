//! Bounded SSE decoding; HTTP 200 and EOF are never completion evidence.
use crate::{error::RuntimeError, openrouter::provider_status};
use reqwest::StatusCode;
use serde_json::Value;

const MAX_EVENT: usize = 256 * 1024;
const MAX_STREAM: usize = 64 * 1024 * 1024;

#[derive(Clone, PartialEq)]
pub enum StreamFrame {
    Chunk(Value),
    Usage(Value),
    Completed,
}

impl std::fmt::Debug for StreamFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Chunk(_) => "Chunk([private provider payload])",
            Self::Usage(_) => "Usage([private provider payload])",
            Self::Completed => "Completed",
        })
    }
}

/// Provider payloads are not logged or passed as client-visible errors.
pub struct Decoder {
    buffer: Vec<u8>,
    data: Vec<u8>,
    received: usize,
    model: String,
    generation: Option<String>,
    finish: Option<String>,
    usage_received: bool,
    done: bool,
    failed: bool,
}

impl Decoder {
    pub fn new(model: &str) -> Result<Self, RuntimeError> {
        if model.is_empty() || model.len() > 256 || model.chars().any(char::is_whitespace) {
            return Err(RuntimeError::InvalidRequest);
        }
        Ok(Self {
            buffer: Vec::new(),
            data: Vec::new(),
            received: 0,
            model: model.into(),
            generation: None,
            finish: None,
            usage_received: false,
            done: false,
            failed: false,
        })
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<StreamFrame>, RuntimeError> {
        if self.failed {
            return Err(RuntimeError::Protocol);
        }
        let result = self.feed_inner(bytes);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn feed_inner(&mut self, bytes: &[u8]) -> Result<Vec<StreamFrame>, RuntimeError> {
        self.received = self
            .received
            .checked_add(bytes.len())
            .ok_or(RuntimeError::Protocol)?;
        if self.received > MAX_STREAM {
            return Err(RuntimeError::Protocol);
        }
        let mut frames = Vec::new();
        // Accumulate a bounded line, including when the network yields a huge chunk.
        for byte in bytes {
            if self.done && !byte.is_ascii_whitespace() {
                return Err(RuntimeError::Protocol);
            }
            if *byte == b'\n' {
                let mut line = std::mem::take(&mut self.buffer);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.is_empty() {
                    if !self.data.is_empty() {
                        let data = std::mem::take(&mut self.data);
                        frames.push(self.event(&data)?);
                    }
                } else if line.starts_with(b"data:") {
                    let payload = line[5..].strip_prefix(b" ").unwrap_or(&line[5..]);
                    if !self.data.is_empty() {
                        self.data.push(b'\n');
                    }
                    if self.data.len() + payload.len() > MAX_EVENT {
                        return Err(RuntimeError::Protocol);
                    }
                    self.data.extend_from_slice(payload);
                } else if !line.starts_with(b":")
                    && !line.starts_with(b"event:")
                    && !line.starts_with(b"id:")
                    && !line.starts_with(b"retry:")
                {
                    return Err(RuntimeError::Protocol);
                }
            } else {
                if self.buffer.len() >= MAX_EVENT {
                    return Err(RuntimeError::Protocol);
                }
                self.buffer.push(*byte);
            }
        }
        Ok(frames)
    }

    fn event(&mut self, data: &[u8]) -> Result<StreamFrame, RuntimeError> {
        if self.done {
            return Err(RuntimeError::Protocol);
        }
        if data == b"[DONE]" {
            if self.finish.is_none() || !self.usage_received || self.generation.is_none() {
                return Err(RuntimeError::Protocol);
            }
            self.done = true;
            return Ok(StreamFrame::Completed);
        }
        let value: Value = serde_json::from_slice(data).map_err(|_| RuntimeError::Protocol)?;
        if let Some(error) = value.get("error") {
            // Numeric error codes use documented HTTP meanings; opaque text stays opaque.
            return Err(error["code"]
                .as_u64()
                .and_then(|code| u16::try_from(code).ok())
                .and_then(|code| StatusCode::from_u16(code).ok())
                .and_then(|status| provider_status(status).err())
                .unwrap_or(RuntimeError::Protocol));
        }
        if value["model"].as_str() != Some(self.model.as_str()) {
            return Err(RuntimeError::Protocol);
        }
        let id = value["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .ok_or(RuntimeError::Protocol)?;
        match &self.generation {
            Some(existing) if existing != id => return Err(RuntimeError::Protocol),
            None => self.generation = Some(id.into()),
            _ => {}
        }
        let choices = value["choices"].as_array().ok_or(RuntimeError::Protocol)?;
        let usage = value.get("usage").filter(|value| !value.is_null());
        if choices.len() > 1 || (choices.is_empty() && usage.is_none()) {
            return Err(RuntimeError::Protocol);
        }
        if let Some(choice) = choices.first() {
            if choice["index"].as_u64() != Some(0) {
                return Err(RuntimeError::Protocol);
            }
            let delta = &choice["delta"];
            if !delta.is_object() {
                return Err(RuntimeError::Protocol);
            }
            let carries_output = delta.as_object().unwrap().iter().any(|(key, value)| {
                key != "role"
                    && match value {
                        Value::Null => false,
                        Value::String(text) => !text.is_empty(),
                        Value::Array(values) => !values.is_empty(),
                        Value::Object(values) => !values.is_empty(),
                        _ => true,
                    }
            });
            if self.finish.is_some() && carries_output {
                return Err(RuntimeError::Protocol);
            }
            if let Some(reason) = choice["finish_reason"].as_str() {
                if reason == "length" {
                    return Err(RuntimeError::OutputTruncated);
                }
                if !matches!(reason, "stop" | "tool_calls") {
                    return Err(RuntimeError::Protocol);
                }
                match &self.finish {
                    Some(previous) if previous != reason || usage.is_none() => {
                        return Err(RuntimeError::Protocol);
                    }
                    None => self.finish = Some(reason.into()),
                    _ => {}
                }
            }
        }
        if let Some(usage) = usage {
            if self.usage_received
                || self.finish.is_none()
                || !usage.is_object()
                || usage["prompt_tokens"].as_u64().is_none()
                || usage["completion_tokens"].as_u64().is_none()
            {
                return Err(RuntimeError::Protocol);
            }
            self.usage_received = true;
            // Retain envelope/finish even when the only finish arrives with usage.
            return Ok(StreamFrame::Usage(value));
        }
        Ok(StreamFrame::Chunk(value))
    }

    pub fn finish(&self) -> Result<(), RuntimeError> {
        if self.done
            && !self.failed
            && self.data.is_empty()
            && self.buffer.iter().all(u8::is_ascii_whitespace)
        {
            Ok(())
        } else {
            Err(RuntimeError::Protocol)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(content: &str, finish: Option<&str>, usage: bool) -> String {
        let mut value = json!({"id":"gen-own","model":"deepseek/deepseek-v4.1-flash",
            "choices":[{"index":0,"delta":{"content":content},"finish_reason":finish}]});
        if usage {
            value["usage"] = json!({"prompt_tokens":20,"completion_tokens":5});
        }
        format!("data: {value}\r\n\r\n")
    }
    fn decoder() -> Decoder {
        Decoder::new("deepseek/deepseek-v4.1-flash").unwrap()
    }

    #[test]
    fn fragmented_utf8_comments_and_repeated_accounting_terminal_are_supported() {
        let mut decoder = decoder();
        let stream = format!(
            ": PROCESSING\r\n\r\n{}{}{}data: [DONE]\r\n\r\n",
            event("Привет", None, false),
            event("", Some("stop"), false),
            event("", Some("stop"), true)
        );
        let mut frames = Vec::new();
        for byte in stream.as_bytes() {
            frames.extend(decoder.feed(&[*byte]).unwrap());
        }
        assert_eq!(frames.len(), 4);
        assert!(matches!(frames[2], StreamFrame::Usage(_)));
        assert_eq!(frames[3], StreamFrame::Completed);
        assert!(decoder.finish().is_ok());
    }

    #[test]
    fn error_after_http_success_never_becomes_completed_and_text_is_not_echoed() {
        let mut decoder = decoder();
        decoder
            .feed(event("Partial output", None, false).as_bytes())
            .unwrap();
        assert_eq!(
            decoder
                .feed(b"data: {\"error\":{\"code\":429,\"message\":\"sensitive quota text\"}}\n\n"),
            Err(RuntimeError::Quota)
        );
        assert_eq!(
            decoder.feed(b"data: [DONE]\n\n"),
            Err(RuntimeError::Protocol)
        );
        assert_eq!(decoder.finish(), Err(RuntimeError::Protocol));
    }

    #[test]
    fn eof_done_without_terminal_usage_wrong_model_and_truncated_output_fail() {
        let mut early = decoder();
        assert_eq!(early.feed(b"data: [DONE]\n\n"), Err(RuntimeError::Protocol));
        let mut partial = decoder();
        partial
            .feed(event("Partial", None, false).as_bytes())
            .unwrap();
        assert_eq!(partial.finish(), Err(RuntimeError::Protocol));
        let mut limited = decoder();
        assert_eq!(
            limited.feed(event("", Some("length"), false).as_bytes()),
            Err(RuntimeError::OutputTruncated)
        );
        let mut foreign = decoder();
        assert_eq!(
            foreign.feed(
                event("x", None, false)
                    .replace("deepseek/deepseek-v4.1-flash", "other/model")
                    .as_bytes()
            ),
            Err(RuntimeError::Protocol)
        );
        let mut no_usage = decoder();
        no_usage
            .feed(event("", Some("stop"), false).as_bytes())
            .unwrap();
        assert_eq!(
            no_usage.feed(b"data: [DONE]\n\n"),
            Err(RuntimeError::Protocol)
        );
    }

    #[test]
    fn multiline_events_are_bounded_and_final_content_cannot_resume() {
        let mut decoder = decoder();
        decoder
            .feed(b"data: {\"id\":\"gen-own\",\"model\":\"deepseek/deepseek-v4.1-flash\",\n")
            .unwrap();
        decoder
            .feed(b"data: \"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n")
            .unwrap();
        assert_eq!(
            decoder.feed(event("Late output", None, false).as_bytes()),
            Err(RuntimeError::Protocol)
        );
        let mut huge = Decoder::new("own/model").unwrap();
        assert_eq!(
            huge.feed(&vec![b'x'; MAX_EVENT + 1]),
            Err(RuntimeError::Protocol)
        );
        assert!(huge.buffer.len() <= MAX_EVENT);
    }
}
