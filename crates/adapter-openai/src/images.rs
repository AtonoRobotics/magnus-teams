//! Pictures, for the kernel's `image` request, with the adapter's own
//! sign-in (an API key, or a ChatGPT sign-in refreshed here). `image_api`
//! picks the provider's route:
//! - `images`: POST `{base}/images/generations`, or `/images/edits` with the
//!   picture to change as a data URL; JSON, answered `data[0].b64_json`.
//!   The OpenAI Images shape, and Codex CLI's own route on the ChatGPT
//!   backend (`codex-rs/ext/image-generation`, 2026: `gpt-image-2`).
//! - `responses`: POST `{base}/responses`, streamed, with the hosted
//!   `image_generation` tool forced; the picture is the
//!   `image_generation_call` item's `result`. `image_host_model` names the
//!   model that runs the tool.
//!
//! A Responses adapter uses `images` unless told otherwise; a Chat one
//! makes no pictures unless `image_api` is set. The fast picture asks for
//! `medium` quality and the premium one for `high`.

use crate::{post_signed_in, Sse};
use magnus_adapter_proto::{ErrorClass, Event, ImageParams, Picture, Sink};
use serde_json::{json, Value};
use std::time::Duration;

/// How long one picture may take at the provider.
const IMAGE_TIMEOUT: Duration = Duration::from_secs(180);
const FAST_QUALITY: &str = "medium";
const PREMIUM_QUALITY: &str = "high";
const HOST_INSTRUCTIONS: &str = "Make the picture the person describes with the image generation tool. Do not ask questions.";
pub(crate) const CANNOT: &str = "this adapter cannot make images: set image_api in its settings to \"images\" or \"responses\"";

/// The provider route for pictures, if this adapter makes them.
fn route(settings: &Value) -> Option<&str> {
    settings["image_api"].as_str().or_else(|| (settings["api"].as_str() == Some("responses")).then_some("images"))
}

fn quality(p: &ImageParams) -> &'static str {
    if p.premium {
        PREMIUM_QUALITY
    } else {
        FAST_QUALITY
    }
}

fn data_url(s: &Picture) -> String {
    format!("data:{};base64,{}", s.media_type, s.data_b64)
}

/// The path and body one picture sends.
pub(crate) fn request_body(p: &ImageParams) -> Result<(&'static str, Value), String> {
    match route(&p.settings) {
        Some("images") => Ok(match &p.source {
            Some(s) => ("/images/edits", json!({ "model": p.model, "prompt": p.prompt, "quality": quality(p), "images": [{ "image_url": data_url(s) }] })),
            None => ("/images/generations", json!({ "model": p.model, "prompt": p.prompt, "quality": quality(p) })),
        }),
        Some("responses") => {
            let host = p.settings["image_host_model"].as_str().ok_or("image_api \"responses\" needs image_host_model, the model that runs the image tool")?;
            let mut content = vec![json!({ "type": "input_text", "text": p.prompt })];
            if let Some(s) = &p.source {
                content.push(json!({ "type": "input_image", "image_url": data_url(s) }));
            }
            Ok((
                "/responses",
                json!({
                    "model": host,
                    "instructions": HOST_INSTRUCTIONS,
                    "input": [{ "type": "message", "role": "user", "content": content }],
                    "tools": [{ "type": "image_generation", "model": p.model, "quality": quality(p), "output_format": "png" }],
                    "tool_choice": { "type": "image_generation" },
                    "stream": true,
                    "store": false,
                }),
            ))
        }
        Some(other) => Err(format!("unknown image_api {other:?}: want \"images\" or \"responses\"")),
        None => Err(CANNOT.into()),
    }
}

/// The picture in an Images reply.
pub(crate) fn in_reply(v: &Value) -> Result<String, (ErrorClass, String)> {
    v["data"][0]["b64_json"].as_str().map(String::from).ok_or((ErrorClass::Malformed, "the provider returned no image".into()))
}

/// What one Responses stream event says of the picture: it, the failure
/// that ended the stream, or nothing yet.
pub(crate) fn in_event(v: &Value) -> Option<Result<String, (ErrorClass, String)>> {
    let made = |item: &Value| (item["type"] == "image_generation_call").then(|| item["result"].as_str().filter(|r| !r.is_empty()).map(String::from)).flatten();
    match v["type"].as_str().unwrap_or_default() {
        "response.output_item.done" => made(&v["item"]).map(Ok),
        "response.completed" | "response.done" | "response.incomplete" => {
            let found = v["response"]["output"].as_array().into_iter().flatten().find_map(made);
            Some(found.ok_or((ErrorClass::Malformed, "the response finished with no image".into())))
        }
        "response.failed" | "error" => {
            let e = if v["type"] == "error" { v } else { &v["response"]["error"] };
            let e = if e["error"].is_object() { &e["error"] } else { e };
            let code = e["code"].as_str().unwrap_or_default();
            let class = if code.contains("limit") || code.contains("quota") { ErrorClass::UsageLimit } else { ErrorClass::Unavailable };
            Some(Err((class, format!("the image failed: {code} {}", e["message"].as_str().unwrap_or_default()))))
        }
        _ => None,
    }
}

pub async fn make(http: &reqwest::Client, p: ImageParams, sink: &mut Sink) -> anyhow::Result<()> {
    let (path, body) = match request_body(&p) {
        Ok(b) => b,
        Err(m) => return sink.error(ErrorClass::InvalidRequest, m).await,
    };
    let streamed = path == "/responses";
    let shape = |r: reqwest::RequestBuilder| {
        let r = r.timeout(IMAGE_TIMEOUT);
        if streamed {
            r.header("accept", "text/event-stream")
        } else {
            r
        }
    };
    let Some(resp) = post_signed_in(http, &p.settings, &p.credential, path, &body, shape, sink).await? else {
        return Ok(());
    };
    let made = if streamed {
        let mut stream = Sse::new(resp);
        loop {
            match stream.next().await {
                None => break Err((ErrorClass::Unavailable, "the stream ended before the image was made".to_string())),
                Some(Err(e)) => break Err(e),
                Some(Ok(v)) => {
                    if let Some(r) = in_event(&v) {
                        break r;
                    }
                }
            }
        }
    } else {
        match resp.json::<Value>().await {
            Ok(v) => in_reply(&v),
            Err(e) => Err((ErrorClass::Malformed, format!("the provider's reply is not JSON: {e}"))),
        }
    };
    match made {
        Ok(data_b64) => sink.send(&Event::Image(Picture { media_type: "image/png".into(), data_b64 })).await,
        Err((class, message)) => sink.error(class, message).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(settings: Value, source: Option<Picture>, premium: bool) -> ImageParams {
        ImageParams { model: "gpt-image-2".into(), settings, credential: None, prompt: "a red barn".into(), source, premium }
    }

    fn source() -> Option<Picture> {
        Some(Picture { media_type: "image/png".into(), data_b64: "AAAA".into() })
    }

    /// A Responses adapter (the Codex sign-in) makes pictures by the route
    /// Codex CLI uses: the Images shape on its own base URL.
    #[test]
    fn a_responses_adapter_asks_the_images_route() {
        let codex = json!({ "api": "responses" });
        let (path, body) = request_body(&params(codex.clone(), None, false)).unwrap();
        assert_eq!((path, body), ("/images/generations", json!({ "model": "gpt-image-2", "prompt": "a red barn", "quality": "medium" })));
        let (path, body) = request_body(&params(codex, source(), true)).unwrap();
        assert_eq!(path, "/images/edits");
        assert_eq!(body["quality"], "high");
        assert_eq!(body["images"], json!([{ "image_url": "data:image/png;base64,AAAA" }]));
    }

    #[test]
    fn a_chat_adapter_makes_no_pictures_unless_told() {
        assert_eq!(request_body(&params(json!({ "api": "chat" }), None, false)).unwrap_err(), CANNOT);
        assert_eq!(request_body(&params(json!({ "api": "chat", "image_api": "images" }), None, false)).unwrap().0, "/images/generations");
        assert!(request_body(&params(json!({ "image_api": "paint" }), None, false)).unwrap_err().contains("paint"));
    }

    /// The hosted tool route forces the tool and runs it on the named host model.
    #[test]
    fn the_hosted_tool_route_forces_the_tool() {
        assert!(request_body(&params(json!({ "image_api": "responses" }), None, false)).unwrap_err().contains("image_host_model"));
        let (path, body) = request_body(&params(json!({ "image_api": "responses", "image_host_model": "gpt-5.5" }), source(), true)).unwrap();
        assert_eq!(path, "/responses");
        assert_eq!(body["model"], "gpt-5.5");
        assert_eq!(body["tools"], json!([{ "type": "image_generation", "model": "gpt-image-2", "quality": "high", "output_format": "png" }]));
        assert_eq!(body["tool_choice"], json!({ "type": "image_generation" }));
        assert_eq!((body["stream"].clone(), body["store"].clone()), (json!(true), json!(false)));
        assert_eq!(body["input"][0]["content"][1], json!({ "type": "input_image", "image_url": "data:image/png;base64,AAAA" }));
    }

    #[test]
    fn the_picture_is_read_from_either_route() {
        assert_eq!(in_reply(&json!({ "data": [{ "b64_json": "UE5H" }] })).unwrap(), "UE5H");
        assert_eq!(in_reply(&json!({ "data": [] })).unwrap_err().0, ErrorClass::Malformed);

        assert!(in_event(&json!({ "type": "response.output_text.delta", "delta": "x" })).is_none());
        assert!(in_event(&json!({ "type": "response.output_item.done", "item": { "type": "reasoning" } })).is_none());
        let done = json!({ "type": "response.output_item.done", "item": { "type": "image_generation_call", "status": "completed", "result": "UE5H" } });
        assert_eq!(in_event(&done).unwrap().unwrap(), "UE5H");
        let completed = json!({ "type": "response.completed", "response": { "output": [{ "type": "image_generation_call", "result": "UE5I" }] } });
        assert_eq!(in_event(&completed).unwrap().unwrap(), "UE5I");
        let empty = json!({ "type": "response.completed", "response": { "output": [] } });
        assert_eq!(in_event(&empty).unwrap().unwrap_err().0, ErrorClass::Malformed);
        let limited = json!({ "type": "response.failed", "response": { "error": { "code": "usage_limit_reached", "message": "later" } } });
        assert_eq!(in_event(&limited).unwrap().unwrap_err().0, ErrorClass::UsageLimit);
    }
}
