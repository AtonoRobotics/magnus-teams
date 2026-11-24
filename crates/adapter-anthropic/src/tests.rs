//! A turn that fails partway still says what Anthropic charged for it, so
//! the kernel books the failed call ("charged, then failed").

use super::*;
use magnus_adapter_proto::testing::{one_turn, provider_once};
use magnus_adapter_proto::CompleteParams;

const SSE: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream";

/// The prompt Anthropic counted at `message_start`.
const START: &str =
    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-test\",\"usage\":{\"input_tokens\":1000,\"output_tokens\":1}}}\n\n";

async fn turn(body: String, chunked: bool) -> Vec<Event> {
    let addr = provider_once(SSE, body, chunked).await.unwrap();
    let settings = json!({ "base_url": format!("http://{addr}") });
    let p = CompleteParams { model: "claude-sonnet-test".into(), settings, credential: Some("key".into()), system: String::new(), messages: vec![], tools: vec![], max_tokens: 10 };
    one_turn(move |req, sink| handle(reqwest::Client::new(), req, sink), p).await.unwrap()
}

/// The usage sent before the turn's error, and the error's class.
fn charged_then(events: &[Event]) -> (Option<Usage>, Option<ErrorClass>) {
    let usage = events.iter().find_map(|e| match e {
        Event::Usage(u) => Some(u.clone()),
        _ => None,
    });
    let error = match events.last() {
        Some(Event::Error { class, .. }) => Some(*class),
        _ => None,
    };
    (usage, error)
}

fn assert_charged(events: &[Event], class: ErrorClass) {
    let (usage, error) = charged_then(events);
    assert_eq!(error, Some(class), "{events:?}");
    let Some(u) = usage else { panic!("the failed turn said nothing of what it cost: {events:?}") };
    assert_eq!(u.input_tokens, 1000);
    // 1000 input tokens and 1 output at $2 and $10 a million.
    assert!((u.cost_usd - 0.00201).abs() < 1e-9, "{}", u.cost_usd);
}

/// Anthropic answers "overloaded" partway through a reply.
#[tokio::test]
async fn an_overloaded_stream_says_what_it_cost() {
    let body = format!("{START}event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}}}\n\n");
    assert_charged(&turn(body, false).await, ErrorClass::Unavailable);
}

/// A tool call whose input is not JSON.
#[tokio::test]
async fn a_tool_call_that_is_not_json_says_what_it_cost() {
    let body = format!(
        "{START}data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"x\"}}}}\n\n\
         data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"input_json_delta\",\"partial_json\":\"{{not json\"}}}}\n\n\
         data: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n"
    );
    assert_charged(&turn(body, false).await, ErrorClass::Malformed);
}

/// The stream breaks after the prompt was counted.
#[tokio::test]
async fn a_broken_stream_says_what_it_cost() {
    assert_charged(&turn(START.to_string(), true).await, ErrorClass::Unavailable);
}

/// A stream that broke before anything was counted sends no usage.
#[tokio::test]
async fn a_stream_that_broke_before_any_count_sends_none() {
    let (usage, error) = charged_then(&turn("data: {\"type\":\"ping\"}\n\n".to_string(), true).await);
    assert_eq!((usage.map(|u| u.input_tokens), error), (None, Some(ErrorClass::Unavailable)));
}
