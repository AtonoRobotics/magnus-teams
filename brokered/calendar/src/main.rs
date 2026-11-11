//! Google Calendar, brokered (connectors spec, Brokered API connectors;
//! Google's hosted Calendar server refuses Magnus with 403, as Gmail's
//! did). An MCP server over Calendar's REST API
//! (`https://www.googleapis.com/calendar/v3`), the `/init` of its own
//! microVM for each person: each call comes with the person's access
//! token, goes to Google through the appliance's egress proxy over TLS
//! checked here, and leaves nothing behind. Its tools are those a person
//! uses in Calendar: see their calendars and events, find a time that
//! works, and add, change, cancel or answer an event. Every tool is
//! classified in the catalog manifest (`catalog/google-calendar.toml`);
//! each that changes an event asks the person first, since the people
//! invited are told.

use magnus_brokered_google::{enc, list, opt_num, query, s, Api};
use magnus_brokered_kit::{serve, Failure, Tool};
use serde_json::{json, Value};

/// `calendar` alone: the one Calendar scope on the project's consent
/// screen, and among the scopes of every method the tools use.
const API: Api = Api { host: "www.googleapis.com", base: "/calendar/v3", name: "Google Calendar", scope: "https://www.googleapis.com/auth/calendar" };

/// The calendar a call is about: the one named, or the person's own.
fn cal(a: &Value) -> String {
    enc(a["calendar_id"].as_str().filter(|c| !c.is_empty()).unwrap_or("primary"))
}

fn event_id(a: &Value) -> Result<String, String> {
    let v = s(a, "event_id");
    if v.is_empty() {
        return Err("event_id is missing".into());
    }
    Ok(enc(&v))
}

/// Whether the people invited are told (`sendUpdates`): yes, unless the
/// call says not.
fn updates(a: &Value) -> &'static str {
    if a["notify"] == false {
        "none"
    } else {
        "all"
    }
}

/// An event as a person reads it.
fn event(e: &Value) -> Value {
    let when = |w: &Value| w["dateTime"].as_str().or_else(|| w["date"].as_str()).unwrap_or_default().to_string();
    let attendees: Vec<Value> = e["attendees"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| json!({ "email": p["email"], "response": p["responseStatus"], "organizer": p["organizer"] == true, "me": p["self"] == true }))
        .collect();
    json!({
        "event_id": e["id"], "summary": e["summary"].as_str().unwrap_or_default(), "description": e["description"].as_str().unwrap_or_default(),
        "location": e["location"].as_str().unwrap_or_default(), "start": when(&e["start"]), "end": when(&e["end"]), "all_day": e["start"]["date"].is_string(),
        "time_zone": e["start"]["timeZone"], "status": e["status"], "organizer": e["organizer"]["email"], "attendees": attendees,
        "recurring_event_id": e["recurringEventId"], "link": e["htmlLink"], "changed": e["updated"],
    })
}

/// The fields of an event a call sets: only those it names.
fn event_body(a: &Value) -> Value {
    let mut b = json!({});
    for (k, field) in [("summary", "summary"), ("description", "description"), ("location", "location")] {
        if let Some(v) = a[k].as_str() {
            b[field] = json!(v);
        }
    }
    for (k, field) in [("start_time", "start"), ("end_time", "end")] {
        if let Some(t) = a[k].as_str() {
            b[field] = if a["all_day"] == true {
                json!({ "date": t.get(..10).unwrap_or(t) })
            } else {
                let mut w = json!({ "dateTime": t });
                if let Some(z) = a["time_zone"].as_str() {
                    w["timeZone"] = json!(z);
                }
                w
            };
        }
    }
    if a.get("attendees").is_some() {
        b["attendees"] = json!(list(a, "attendees").iter().map(|e| json!({ "email": e })).collect::<Vec<_>>());
    }
    b
}

/// The most pages of calendars one call reads (250 each: 5,000 calendars).
const PAGES: usize = 20;

/// Every calendar the person has, page after page, up to `PAGES`; whether
/// there were more is said.
fn list_calendars(_: &Value, t: &str) -> Result<Value, Failure> {
    let (mut cals, mut next): (Vec<Value>, Option<String>) = (vec![], None);
    for _ in 0..PAGES {
        let r = API.call("GET", &format!("/users/me/calendarList{}", query(&[("maxResults", Some("250".into())), ("pageToken", next.take())])), t, None)?;
        cals.extend(r["items"].as_array().into_iter().flatten().map(|c| json!({ "calendar_id": c["id"], "name": c["summaryOverride"].as_str().or(c["summary"].as_str()), "primary": c["primary"] == true, "access": c["accessRole"], "time_zone": c["timeZone"] })));
        next = r["nextPageToken"].as_str().map(String::from);
        if next.is_none() {
            break;
        }
    }
    Ok(json!({ "calendars": cals, "more": next.is_some() }))
}

fn events(a: &Value, t: &str, q: Option<String>) -> Result<Value, Failure> {
    let pairs = [
        ("singleEvents", Some("true".to_string())),
        ("orderBy", Some("startTime".to_string())),
        ("q", q),
        ("timeMin", a["start_time"].as_str().map(String::from)),
        ("timeMax", a["end_time"].as_str().map(String::from)),
        ("updatedMin", a["changed_since"].as_str().map(String::from)),
        ("timeZone", a["time_zone"].as_str().map(String::from)),
        ("maxResults", opt_num(a, "max_results")),
        ("pageToken", a["page_token"].as_str().map(String::from)),
    ];
    let r = API.call("GET", &format!("/calendars/{}/events{}", cal(a), query(&pairs)), t, None)?;
    Ok(json!({ "events": r["items"].as_array().into_iter().flatten().map(event).collect::<Vec<_>>(), "next_page_token": r["nextPageToken"].as_str() }))
}

fn list_events(a: &Value, t: &str) -> Result<Value, Failure> {
    events(a, t, None)
}

fn search_events(a: &Value, t: &str) -> Result<Value, Failure> {
    events(a, t, Some(s(a, "query")))
}

fn get_event(a: &Value, t: &str) -> Result<Value, Failure> {
    Ok(event(&API.call("GET", &format!("/calendars/{}/events/{}", cal(a), event_id(a)?), t, None)?))
}

fn create_event(a: &Value, t: &str) -> Result<Value, Failure> {
    Ok(event(&API.call("POST", &format!("/calendars/{}/events?sendUpdates={}", cal(a), updates(a)), t, Some(&event_body(a)))?))
}

fn update_event(a: &Value, t: &str) -> Result<Value, Failure> {
    Ok(event(&API.call("PATCH", &format!("/calendars/{}/events/{}?sendUpdates={}", cal(a), event_id(a)?, updates(a)), t, Some(&event_body(a)))?))
}

fn delete_event(a: &Value, t: &str) -> Result<Value, Failure> {
    API.call("DELETE", &format!("/calendars/{}/events/{}?sendUpdates={}", cal(a), event_id(a)?, updates(a)), t, None)?;
    Ok(json!({ "deleted": true }))
}

/// The event's people with the person's own answer set: every one kept,
/// as the list is replaced whole.
fn answered(e: &Value, response: &str, comment: Option<&str>) -> Result<Value, String> {
    let mut people = e["attendees"].as_array().cloned().unwrap_or_default();
    let me = people.iter_mut().find(|p| p["self"] == true).ok_or("The person is not invited to that event, so there is no answer to give. Nothing was changed.")?;
    me["responseStatus"] = json!(response);
    if let Some(c) = comment {
        me["comment"] = json!(c);
    }
    Ok(json!({ "attendees": people }))
}

fn respond_to_event(a: &Value, t: &str) -> Result<Value, Failure> {
    let path = format!("/calendars/{}/events/{}", cal(a), event_id(a)?);
    let e = API.call("GET", &path, t, None)?;
    let body = answered(&e, &s(a, "response"), a["comment"].as_str())?;
    Ok(event(&API.call("PATCH", &format!("{path}?sendUpdates={}", updates(a)), t, Some(&body))?))
}

/// Seconds since 1970 of an RFC 3339 time (`2026-11-04T10:00:00Z`,
/// `...T10:00:00.000-08:00`).
fn epoch(t: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| t.get(r).and_then(|x| x.parse::<i64>().ok());
    let (y, mo, d, h, mi, sec) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let rest = t.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match rest {
        "Z" | "z" => 0,
        o if o.len() == 6 => (if o.starts_with('-') { -1 } else { 1 }) * (o.get(1..3)?.parse::<i64>().ok()? * 3600 + o.get(4..6)?.parse::<i64>().ok()? * 60),
        _ => return None,
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let (y, m) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + (153 * m + 2) / 5 + d - 1 - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + sec - offset)
}

fn rfc3339(e: i64) -> String {
    let (days, secs) = (e.div_euclid(86400), e.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// Up to `max` times of `minutes` between `from` and `to`, on the quarter
/// hour, that no busy time overlaps.
fn free(from: i64, to: i64, minutes: i64, mut busy: Vec<(i64, i64)>, max: usize) -> Vec<(i64, i64)> {
    busy.sort();
    let (len, mut at, mut out) = (minutes * 60, from, vec![]);
    while out.len() < max {
        at = (at + 899).div_euclid(900) * 900;
        if at + len > to {
            break;
        }
        match busy.iter().find(|(s, e)| *s < at + len && *e > at) {
            Some((_, e)) => at = *e,
            None => {
                out.push((at, at + len));
                at += len;
            }
        }
    }
    out
}

/// When each calendar asked about is busy, and which could not be checked:
/// a calendar with errors, or a group whose members Google could not list
/// (`groupTooBig`, `notFound`), reported apart from the calendars.
fn busy_times(r: &Value) -> (Vec<(i64, i64)>, Vec<String>) {
    let (mut busy, mut unknown) = (vec![], vec![]);
    let failed = |x: &Value| x["errors"].as_array().is_some_and(|e| !e.is_empty());
    for (id, c) in r["calendars"].as_object().into_iter().flatten() {
        if failed(c) {
            unknown.push(id.clone());
        }
        busy.extend(c["busy"].as_array().into_iter().flatten().filter_map(|b| Some((epoch(b["start"].as_str()?)?, epoch(b["end"].as_str()?)?))));
    }
    unknown.extend(r["groups"].as_object().into_iter().flatten().filter(|(_, g)| failed(g)).map(|(id, _)| id.clone()));
    (busy, unknown)
}

fn suggest_time(a: &Value, t: &str) -> Result<Value, Failure> {
    let (from, to) = (s(a, "start_time"), s(a, "end_time"));
    let (Some(f), Some(e)) = (epoch(&from), epoch(&to)) else { return Err("start_time and end_time are RFC 3339 times, such as 2026-11-04T09:00:00-08:00".to_string().into()) };
    let mut who = list(a, "attendees");
    who.insert(0, "primary".into());
    let body = json!({ "timeMin": from, "timeMax": to, "items": who.iter().map(|id| json!({ "id": id })).collect::<Vec<_>>() });
    let (busy, unknown) = busy_times(&API.ask("/freeBusy", t, &body)?);
    let max = a["max_suggestions"].as_u64().unwrap_or(3).clamp(1, 10) as usize;
    let slots = free(f, e, a["duration_minutes"].as_i64().unwrap_or(30).max(5), busy, max);
    Ok(json!({ "suggestions": slots.iter().map(|(s, e)| json!({ "start": rfc3339(*s), "end": rfc3339(*e) })).collect::<Vec<_>>(), "not_checked": unknown }))
}

/// An object schema: its properties, which are required, and nothing else.
fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn tools() -> Vec<Tool> {
    let st = json!({ "type": "string" });
    let n = json!({ "type": "integer" });
    let yes = json!({ "type": "boolean" });
    let strs = json!({ "type": "array", "items": { "type": "string" } });
    let maybe = json!({ "type": ["string", "null"] });
    let ev = json!({ "type": "object", "required": ["event_id", "summary", "start", "end", "attendees"] });
    let events = obj(json!({ "events": { "type": "array", "items": ev }, "next_page_token": maybe }), &["events"]);
    let range = json!({ "calendar_id": st, "start_time": st, "end_time": st, "time_zone": st, "max_results": n, "page_token": st, "changed_since": st });
    let mut search = range.clone();
    search["query"] = st.clone();
    let one = obj(json!({ "event_id": st, "calendar_id": st }), &["event_id"]);
    let fields = json!({ "calendar_id": st, "summary": st, "description": st, "location": st, "start_time": st, "end_time": st, "all_day": yes, "time_zone": st, "attendees": strs, "notify": yes });
    let mut change = fields.clone();
    change["event_id"] = st.clone();
    let tool = |name, description, input, output, run| Tool { name, description, input, output, run };
    vec![
        tool(
            "list_calendars",
            "List every one of the person's calendars: their own and those shared with them.",
            obj(json!({}), &[]),
            obj(json!({ "calendars": { "type": "array" }, "more": yes }), &["calendars"]),
            list_calendars,
        ),
        tool("list_events", "List the events on a calendar (the person's own unless named) between two times, RFC 3339, in order; only those changed since a time, cancelled ones included, when changed_since names one.", obj(range, &[]), events.clone(), list_events),
        tool(
            "search_events",
            "Find events whose words match, on a calendar (the person's own unless named), between two times if given.",
            obj(search, &["query"]),
            events,
            search_events,
        ),
        tool("get_event", "Read one event: when, where, who is invited and how each answered.", one.clone(), ev.clone(), get_event),
        tool(
            "suggest_time",
            "Find times of a length between two times when the person and those named (by email) are all free.",
            obj(json!({ "start_time": st, "end_time": st, "duration_minutes": n, "attendees": strs, "max_suggestions": n }), &["start_time", "end_time", "duration_minutes"]),
            obj(json!({ "suggestions": { "type": "array" }, "not_checked": strs }), &["suggestions"]),
            suggest_time,
        ),
        tool("create_event", "Add an event. The people invited are told, unless notify is false.", obj(fields, &["summary", "start_time", "end_time"]), ev.clone(), create_event),
        tool("update_event", "Change an event: only what is given. The people invited are told, unless notify is false.", obj(change, &["event_id"]), ev.clone(), update_event),
        tool(
            "delete_event",
            "Cancel an event. The people invited are told, unless notify is false.",
            obj(json!({ "event_id": st, "calendar_id": st, "notify": yes }), &["event_id"]),
            obj(json!({ "deleted": yes }), &["deleted"]),
            delete_event,
        ),
        tool(
            "respond_to_event",
            "Answer an invitation: accepted, declined or tentative, with a note if given. The organizer is told.",
            obj(
                json!({ "event_id": st, "calendar_id": st, "response": { "type": "string", "enum": ["accepted", "declined", "tentative"] }, "comment": st, "notify": yes }),
                &["event_id", "response"],
            ),
            ev,
            respond_to_event,
        ),
    ]
}

fn main() {
    serve("magnus-calendar", tools())
}

#[cfg(test)]
#[path = "../../../tests/brokered-calendar/calendar.rs"]
mod tests;
