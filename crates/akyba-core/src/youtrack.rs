//! YouTrack's issues as Akyba's tickets, parsed from YouTrack's REST API
//! (<https://www.jetbrains.com/help/youtrack/devportal/youtrack-rest-api.html>), the same for
//! YouTrack Cloud and a server of your own; the HTTP calls live in the app.

use serde::Deserialize;
use serde_json::Value;

use crate::tickets::{Source, Ticket, hex_color};

/// The fields Akyba asks for on each issue: its ids, title, project, tags, and the custom fields
/// it reads its state, priority and due date from.
pub const ISSUE_FIELDS: &str =
    "id,idReadable,summary,project(id,name),tags(name),customFields(name,$type,value(name,ordinal,color(background)))";

/// The open issues assigned to you, the most recently changed first.
pub const MY_OPEN_ISSUES: &str = "for: me #Unresolved sort by: updated desc";

/// How many issues Akyba asks for at a time.
pub const PAGE_SIZE: usize = 100;

/// The first part of a path that names one of YouTrack's pages, not the folder it's served from
/// (the root, or one like `/youtrack`).
const PAGES: [&str; 9] = ["api", "issues", "issue", "dashboard", "agiles", "projects", "admin", "articles", "hub"];

/// What a URL pasted in Settings says about where YouTrack is: `https://` added when it has no
/// scheme, and anything after the address taken off (a page of it, `/api`, a slash), so
/// `youtrack.acme.dev/youtrack/issues?q=x` becomes `https://youtrack.acme.dev/youtrack`.
pub fn instance_url(typed: &str) -> Result<String, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err("Type your YouTrack's address, like https://youtrack.acme.dev.".to_owned());
    }
    let url = if typed.contains("://") { typed.to_owned() } else { format!("https://{typed}") };
    let (scheme, rest) = url.split_once("://").unwrap_or(("https", &url));
    if !matches!(scheme.to_lowercase().as_str(), "https" | "http") {
        return Err(format!("{typed} isn't a web address."));
    }
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host.is_empty() || host.contains(char::is_whitespace) {
        return Err(format!("{typed} isn't a web address."));
    }
    let kept: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty())
        .take_while(|part| !PAGES.contains(&part.to_lowercase().as_str()))
        .collect();
    let path = if kept.is_empty() { String::new() } else { format!("/{}", kept.join("/")) };
    Ok(format!("{}://{host}{path}", scheme.to_lowercase()))
}

/// The web page of an issue.
pub fn issue_url(instance: &str, key: &str) -> String {
    format!("{instance}/issue/{key}")
}

#[derive(Deserialize)]
struct IssueJson {
    id: String,
    #[serde(rename = "idReadable", default)]
    id_readable: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    project: Option<ProjectJson>,
    #[serde(default)]
    tags: Vec<NamedJson>,
    #[serde(rename = "customFields", default)]
    custom_fields: Vec<FieldJson>,
}

#[derive(Deserialize)]
struct ProjectJson {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct NamedJson {
    name: String,
}

#[derive(Deserialize)]
struct FieldJson {
    name: String,
    #[serde(rename = "$type", default)]
    kind: String,
    /// An object for a single value (a state, a priority), a list for several, a number for a
    /// date, or nothing when it's empty.
    #[serde(default)]
    value: Value,
}

impl FieldJson {
    /// Whether this is the field the issue's state is in. Projects name it as they like (State,
    /// Stage, Status), but its type says what it is.
    fn is_state(&self) -> bool {
        matches!(self.kind.as_str(), "StateIssueCustomField" | "StateMachineIssueCustomField")
    }

    fn named(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    /// The value's name, colour and place among the field's values, when it's one value.
    fn element(&self) -> Option<(String, Option<String>, Option<u32>)> {
        let name = self.value.get("name")?.as_str()?.to_owned();
        let color = self
            .value
            .pointer("/color/background")
            .and_then(Value::as_str)
            .and_then(hex_color)
            // YouTrack's "no colour" is a white background, which reads as no dot at all.
            .filter(|color| color != "#ffffff");
        let ordinal = self.value.get("ordinal").and_then(Value::as_u64).and_then(|rank| u32::try_from(rank).ok());
        Some((name, color, ordinal))
    }
}

impl IssueJson {
    fn into_ticket(self, instance: &str) -> Ticket {
        let state = self
            .custom_fields
            .iter()
            .find(|field| field.is_state())
            .or_else(|| self.custom_fields.iter().find(|field| field.named("State")))
            .and_then(FieldJson::element);
        let priority = self.custom_fields.iter().find(|field| field.named("Priority")).and_then(FieldJson::element);
        let due = self
            .custom_fields
            .iter()
            .find(|field| field.named("Due Date") || field.kind == "DateIssueCustomField")
            .and_then(|field| field.value.as_i64());
        let key = self.id_readable.filter(|key| !key.is_empty());
        let url = issue_url(instance, key.as_deref().unwrap_or(&self.id));
        let (status, status_color) = state.map_or_else(|| ("Open".to_owned(), None), |(name, color, _)| (name, color));
        let (priority, priority_rank) = priority.map_or((None, None), |(name, _, rank)| (Some(name), rank));
        let (space_id, space) = self.project.map_or((None, None), |project| (project.id, project.name));
        Ticket {
            source: Source::YouTrack,
            id: self.id,
            custom_id: key,
            title: self.summary.unwrap_or_default(),
            status,
            status_color,
            url,
            list: None,
            space_id,
            space,
            priority,
            priority_rank,
            tags: self.tags.into_iter().map(|tag| tag.name).collect(),
            due,
        }
    }
}

/// A page of `GET /api/issues`: its tickets, linked to `instance`, and whether more may follow.
pub fn parse_issue_page(json: &str, instance: &str) -> Result<(Vec<Ticket>, bool), serde_json::Error> {
    let issues: Vec<IssueJson> = serde_json::from_str(json)?;
    // YouTrack doesn't say whether more follow; a full page may have.
    let more = issues.len() >= PAGE_SIZE;
    Ok((issues.into_iter().map(|issue| issue.into_ticket(instance)).collect(), more))
}

/// Who the token belongs to, from `GET /api/users/me`: their full name, else their login.
pub fn parse_me(json: &str) -> Result<String, serde_json::Error> {
    #[derive(Deserialize)]
    struct Me {
        #[serde(default)]
        login: Option<String>,
        #[serde(rename = "fullName", default)]
        full_name: Option<String>,
    }
    let me: Me = serde_json::from_str(json)?;
    Ok(me.full_name.filter(|name| !name.trim().is_empty()).or(me.login).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r##"[
        {"id": "2-15", "idReadable": "ABC-15", "summary": "Round VAT half up on invoices",
         "project": {"id": "0-1", "name": "Acme Web", "$type": "Project"},
         "tags": [{"name": "billing", "$type": "Tag"}],
         "customFields": [
            {"name": "Priority", "$type": "SingleEnumIssueCustomField",
             "value": {"name": "Critical", "ordinal": 1, "color": {"background": "#E30", "$type": "FieldStyle"},
                       "$type": "EnumBundleElement"}},
            {"name": "Stage", "$type": "StateIssueCustomField",
             "value": {"name": "In Progress", "ordinal": 2, "color": {"background": "#7b68ee"},
                       "$type": "StateBundleElement"}},
            {"name": "Due Date", "$type": "DateIssueCustomField", "value": 1700000000000},
            {"name": "Assignee", "$type": "SingleUserIssueCustomField", "value": {"name": "Jane Doe"}}
         ], "$type": "Issue"},
        {"id": "2-16", "idReadable": "ABC-16", "summary": "Export the customer list",
         "project": {"id": "0-1", "name": "Acme Web"}, "tags": [],
         "customFields": [
            {"name": "Priority", "$type": "SingleEnumIssueCustomField", "value": null},
            {"name": "State", "$type": "StateIssueCustomField",
             "value": {"name": "Open", "color": {"background": "#fff"}}}
         ]}
    ]"##;

    #[test]
    fn a_page_of_issues_becomes_tickets() {
        let (tickets, more) = parse_issue_page(PAGE, "https://youtrack.acme.dev").expect("parses");
        assert!(!more, "a page short of full is the last");
        let first = &tickets[0];
        assert_eq!(first.key(), "ABC-15");
        assert_eq!(first.title, "Round VAT half up on invoices");
        assert_eq!(first.status, "In Progress", "the state is found by its type, whatever it's called");
        assert_eq!(first.status_color.as_deref(), Some("#7b68ee"));
        assert_eq!((first.priority.as_deref(), first.priority_rank), (Some("Critical"), Some(1)));
        assert_eq!(first.space.as_deref(), Some("Acme Web"));
        assert_eq!(first.tags, ["billing"]);
        assert_eq!(first.due, Some(1_700_000_000_000));
        assert_eq!(first.url, "https://youtrack.acme.dev/issue/ABC-15");
        assert_eq!(first.source, Source::YouTrack);
    }

    #[test]
    fn empty_fields_and_no_colour_are_left_out() {
        let (tickets, _) = parse_issue_page(PAGE, "https://youtrack.acme.dev").expect("parses");
        let second = &tickets[1];
        assert_eq!(second.status, "Open");
        assert_eq!(second.status_color, None, "white is YouTrack's no colour");
        assert_eq!(second.priority, None);
        assert_eq!(second.due, None);
    }

    #[test]
    fn an_issue_without_a_state_field_is_open() {
        let json = r#"[{"id": "2-1", "idReadable": "ABC-1", "summary": "s", "customFields": []}]"#;
        let (tickets, _) = parse_issue_page(json, "https://y").expect("parses");
        assert_eq!(tickets[0].status, "Open");
    }

    #[test]
    fn a_full_page_may_have_more() {
        let issue = r#"{"id": "2-1", "idReadable": "ABC-1", "summary": "s"}"#;
        let json = format!("[{}]", vec![issue; PAGE_SIZE].join(","));
        assert!(parse_issue_page(&json, "https://y").expect("parses").1);
    }

    #[test]
    fn the_token_owner_is_named() {
        assert_eq!(parse_me(r#"{"login": "jane", "fullName": "Jane Doe"}"#).expect("parses"), "Jane Doe");
        assert_eq!(parse_me(r#"{"login": "jane", "fullName": ""}"#).expect("parses"), "jane");
    }

    #[test]
    fn a_pasted_address_becomes_the_instance() {
        let url = |typed| instance_url(typed).expect("an address");
        assert_eq!(url("youtrack.acme.dev"), "https://youtrack.acme.dev");
        assert_eq!(url(" https://youtrack.acme.dev/ "), "https://youtrack.acme.dev");
        assert_eq!(url("https://acme.youtrack.cloud/issues?q=for:%20me"), "https://acme.youtrack.cloud");
        assert_eq!(url("https://acme.dev/youtrack/issue/ABC-1"), "https://acme.dev/youtrack");
        assert_eq!(url("http://10.0.0.5:8080/api"), "http://10.0.0.5:8080");
        assert_eq!(url("HTTPS://acme.dev/youtrack/"), "https://acme.dev/youtrack");
        assert!(instance_url("").is_err());
        assert!(instance_url("ftp://acme.dev").is_err());
        assert!(instance_url("https:///issues").is_err());
    }
}
