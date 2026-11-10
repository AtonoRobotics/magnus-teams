//! Outlook calendar, brokered (connectors spec, Brokered API connectors).
//! An MCP server over Microsoft Graph's calendar API
//! (`https://graph.microsoft.com/v1.0/me/events`), the `/init` of its own
//! microVM for each person: each call comes with the person's access
//! token, goes to Graph through the appliance's egress proxy over TLS
//! checked here, and leaves nothing behind. Its tools are those a person
//! uses in Outlook's calendar: see their calendars and events, and add,
//! change or answer an event. Every tool is classified in the catalog
//! manifest (`catalog/microsoft-calendar.toml`); each that changes an event
//! asks the person first, since Outlook tells the people invited.

use magnus_brokered_kit::{serve, Failure, Tool};
use magnus_brokered_microsoft::{enc, id, list, next_page, obj, opt_num, page, query, s, Graph};
use serde_json::{json, Value};

/// `Calendars.ReadWrite` alone: the least privileged delegated permission
/// of every method the tools use (Microsoft Graph permissions reference).
/// It cannot share a calendar.
const API: Graph = Graph { name: "Outlook Calendar", scopes: &["https://graph.microsoft.com/Calendars.ReadWrite"] };

const ME: &str = "/v1.0/me/";

/// The most pages of calendars one call reads (100 each).
const CALENDAR_PAGES_MAX: usize = 20;
/// The most events one page asks for, when the call names none.
const PAGE_SIZE_DEFAULT: u64 = 50;

/// The calendar a call is about: the one named, or the person's own.
fn cal(a: &Value) -> String {
    match a["calendar_id"].as_str().filter(|c| !c.is_empty()) {
        Some(c) => format!("{ME}calendars/{}/", enc(c)),
        None => ME.to_string(),
    }
}

/// An event as a person reads it.
fn event(e: &Value) -> Value {
    let attendees: Vec<Value> =
        e["attendees"].as_array().into_iter().flatten().map(|p| json!({ "email": p["emailAddress"]["address"], "response": p["status"]["response"], "type": p["type"] })).collect();
    json!({
        "event_id": e["id"], "subject": e["subject"].as_str().unwrap_or_default(), "body": e["body"]["content"].as_str().unwrap_or_default(),
        "location": e["location"]["displayName"].as_str().unwrap_or_default(), "start": e["start"]["dateTime"], "end": e["end"]["dateTime"],
        "time_zone": e["start"]["timeZone"], "all_day": e["isAllDay"] == true, "cancelled": e["isCancelled"] == true,
        "organizer": e["organizer"]["emailAddress"]["address"], "is_organizer": e["isOrganizer"] == true,
        "my_response": e["responseStatus"]["response"], "attendees": attendees,
        "series_id": e["seriesMasterId"], "online_meeting": e["onlineMeeting"]["joinUrl"], "link": e["webLink"],
    })
}

/// A time as Graph takes it: a local time and its time zone. A time ending
/// in `Z` is in UTC; one with another offset is refused, as Graph would
/// read it in the zone named and not the offset.
fn when(t: &str, zone: Option<&str>, all_day: bool) -> Result<Value, String> {
    let (local, utc) = match t.strip_suffix('Z').or_else(|| t.strip_suffix('z')) {
        Some(l) => (l, true),
        None => (t, false),
    };
    let time = local.get(10..).unwrap_or_default();
    if time.contains('+') || time.contains('-') {
        return Err(format!("{t} names an offset: give the local time, such as 2026-11-04T10:00:00, and its time_zone"));
    }
    let local = if all_day { format!("{}T00:00:00", local.get(..10).unwrap_or(local)) } else { local.to_string() };
    let zone = if utc { "UTC" } else { zone.unwrap_or("UTC") };
    Ok(json!({ "dateTime": local, "timeZone": zone }))
}

/// The fields of an event a call sets: only those it names.
fn event_body(a: &Value) -> Result<Value, String> {
    let mut b = json!({});
    if let Some(v) = a["subject"].as_str() {
        b["subject"] = json!(v);
    }
    if let Some(v) = a["body"].as_str() {
        b["body"] = json!({ "contentType": "Text", "content": v });
    }
    if let Some(v) = a["location"].as_str() {
        b["location"] = json!({ "displayName": v });
    }
    let all_day = a["all_day"] == true;
    if a.get("all_day").is_some() {
        b["isAllDay"] = json!(all_day);
    }
    for (k, field) in [("start_time", "start"), ("end_time", "end")] {
        if let Some(t) = a[k].as_str() {
            b[field] = when(t, a["time_zone"].as_str(), all_day)?;
        }
    }
    if a.get("attendees").is_some() {
        b["attendees"] = json!(list(a, "attendees").iter().map(|e| json!({ "emailAddress": { "address": e }, "type": "required" })).collect::<Vec<_>>());
    }
    Ok(b)
}

fn list_calendars(_: &Value, t: &str) -> Result<Value, Failure> {
    let (mut cals, mut next): (Vec<Value>, Option<String>) = (vec![], None);
    let mut path = format!("{ME}calendars?$top=100");
    for _ in 0..CALENDAR_PAGES_MAX {
        let r = API.call("GET", &path, t, None)?;
        cals.extend(r["value"].as_array().into_iter().flatten().map(
            |c| json!({ "calendar_id": c["id"], "name": c["name"], "primary": c["isDefaultCalendar"] == true, "can_edit": c["canEdit"] == true, "owner": c["owner"]["address"] }),
        ));
        next = next_page(&r);
        match &next {
            Some(p) => path = page(p, ME)?,
            None => break,
        }
    }
    Ok(json!({ "calendars": cals, "more": next.is_some() }))
}

fn list_events(a: &Value, t: &str) -> Result<Value, Failure> {
    let path = match a["page_token"].as_str().filter(|p| !p.is_empty()) {
        Some(p) => page(p, ME)?,
        None => {
            let pairs = [
                ("startDateTime", Some(s(a, "start_time"))),
                ("endDateTime", Some(s(a, "end_time"))),
                ("$top", opt_num(a, "max_results").or(Some(PAGE_SIZE_DEFAULT.to_string()))),
                ("$orderby", Some("start/dateTime".to_string())),
            ];
            format!("{}calendarView{}", cal(a), query(&pairs))
        }
    };
    let r = API.call("GET", &path, t, None)?;
    Ok(json!({ "events": r["value"].as_array().into_iter().flatten().map(event).collect::<Vec<_>>(), "next_page_token": next_page(&r) }))
}

fn get_event(a: &Value, t: &str) -> Result<Value, Failure> {
    Ok(event(&API.call("GET", &format!("{ME}events/{}", id(a, "event_id")?), t, None)?))
}

fn create_event(a: &Value, t: &str) -> Result<Value, Failure> {
    Ok(event(&API.call("POST", &format!("{}events", cal(a)), t, Some(&event_body(a)?))?))
}

fn update_event(a: &Value, t: &str) -> Result<Value, Failure> {
    Ok(event(&API.call("PATCH", &format!("{ME}events/{}", id(a, "event_id")?), t, Some(&event_body(a)?))?))
}

/// Graph's action for an answer.
fn answer(response: &str) -> Result<&'static str, String> {
    match response {
        "accepted" => Ok("accept"),
        "declined" => Ok("decline"),
        "tentative" => Ok("tentativelyAccept"),
        other => Err(format!("{other} is not accepted, declined or tentative")),
    }
}

fn respond_to_event(a: &Value, t: &str) -> Result<Value, Failure> {
    let how = answer(&s(a, "response"))?;
    let body = json!({ "comment": s(a, "comment"), "sendResponse": a["notify"] != false });
    API.call("POST", &format!("{ME}events/{}/{how}", id(a, "event_id")?), t, Some(&body))?;
    Ok(json!({ "answered": s(a, "response") }))
}

fn tools() -> Vec<Tool> {
    let st = json!({ "type": "string" });
    let n = json!({ "type": "integer" });
    let yes = json!({ "type": "boolean" });
    let strs = json!({ "type": "array", "items": { "type": "string" } });
    let maybe = json!({ "type": ["string", "null"] });
    let ev = json!({ "type": "object", "required": ["event_id", "subject", "start", "end", "attendees"] });
    let fields = json!({ "calendar_id": st, "subject": st, "body": st, "location": st, "start_time": st, "end_time": st, "all_day": yes, "time_zone": st, "attendees": strs });
    let mut change = fields.clone();
    change["event_id"] = st.clone();
    if let Some(o) = change.as_object_mut() {
        o.remove("calendar_id");
    }
    let tool = |name, description, input, output, run| Tool { name, description, input, output, run };
    vec![
        tool(
            "list_calendars",
            "List every one of the person's calendars: their own and those shared with them.",
            obj(json!({}), &[]),
            obj(json!({ "calendars": { "type": "array" }, "more": yes }), &["calendars"]),
            list_calendars,
        ),
        tool(
            "list_events",
            "List the events on a calendar (the person's own unless named) between two times, such as 2026-11-04T00:00:00Z, in order, each meeting of a series apart. Times come back in UTC.",
            obj(json!({ "calendar_id": st, "start_time": st, "end_time": st, "max_results": n, "page_token": st }), &["start_time", "end_time"]),
            obj(json!({ "events": { "type": "array", "items": ev }, "next_page_token": maybe }), &["events"]),
            list_events,
        ),
        tool("get_event", "Read one event: when, where, who is invited and how each answered.", obj(json!({ "event_id": st }), &["event_id"]), ev.clone(), get_event),
        tool(
            "create_event",
            "Add an event. Times are local, such as 2026-11-04T10:00:00, in time_zone (such as Europe/London; UTC unless given). Outlook invites the people named.",
            obj(fields, &["subject", "start_time", "end_time"]),
            ev.clone(),
            create_event,
        ),
        tool("update_event", "Change an event: only what is given. Outlook tells the people invited.", obj(change, &["event_id"]), ev, update_event),
        tool(
            "respond_to_event",
            "Answer an invitation: accepted, declined or tentative, with a note if given. The organizer is told, unless notify is false.",
            obj(
                json!({ "event_id": st, "response": { "type": "string", "enum": ["accepted", "declined", "tentative"] }, "comment": st, "notify": yes }),
                &["event_id", "response"],
            ),
            obj(json!({ "answered": st }), &["answered"]),
            respond_to_event,
        ),
    ]
}

fn main() {
    serve("magnus-outlook-calendar", tools())
}

#[cfg(test)]
#[path = "../../../tests/brokered-outlook-calendar/calendar.rs"]
mod tests;
