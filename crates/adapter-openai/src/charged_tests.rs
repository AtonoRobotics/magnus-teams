//! A turn that fails after the provider counted what it used still says
//! what it cost, so the kernel books the failed call ("charged, then
//! failed"): a reply whose tool arguments are not JSON, on either API.

use super::*;
use magnus_adapter_proto::testing::{one_turn, provider_once};

const SSE: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream";

async fn turn(api: &str, body: &str) -> Vec<Event> {
    let addr = provider_once(SSE, body.to_string(), false).await.unwrap();
    let settings = json!({
        "api": api,
        "base_url": format!("http://{addr}/v1"),
        "auth": { "kind": "api_key" },
        "prices": { "gpt-test": { "input": 2.0, "output": 10.0 } },
    });
    let p = CompleteParams { model: "gpt-test".into(), settings, credential: Some("key".into()), system: String::new(), messages: vec![], tools: vec![], max_tokens: 10 };
    one_turn(move |req, sink| handle(reqwest::Client::new(), req, sink), p).await.unwrap()
}

fn assert_charged(events: &[Event]) {
    assert!(matches!(events.last(), Some(Event::Error { class: ErrorClass::Malformed, .. })), "{events:?}");
    let usage = events.iter().find_map(|e| match e {
        Event::Usage(u) => Some(u.clone()),
        _ => None,
    });
    let Some(u) = usage else { panic!("the failed turn said nothing of what it cost: {events:?}") };
    assert_eq!((u.input_tokens, u.output_tokens), (1000, 20));
    // 1000 input tokens and 20 output at $2 and $10 a million.
    assert!((u.cost_usd - 0.0022).abs() < 1e-9, "{}", u.cost_usd);
}

#[tokio::test]
async fn a_chat_reply_with_arguments_that_are_not_json_says_what_it_cost() {
    let body = "data: {\"model\":\"gpt-test\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"x\",\"arguments\":\"{not json\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n\
                data: {\"model\":\"gpt-test\",\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":20}}\n\n\
                data: [DONE]\n\n";
    assert_charged(&turn("chat", body).await);
}

#[tokio::test]
async fn a_responses_reply_with_arguments_that_are_not_json_says_what_it_cost() {
    let body = "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"c1\",\"name\":\"x\",\"arguments\":\"{not json\"}}\n\n\
                data: {\"type\":\"response.completed\",\"response\":{\"model\":\"gpt-test\",\"usage\":{\"input_tokens\":1000,\"output_tokens\":20}}}\n\n";
    assert_charged(&turn("responses", body).await);
}
