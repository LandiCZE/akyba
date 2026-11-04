//! ClickUp's tasks as Akyba's tickets, parsed from ClickUp's API v2
//! (<https://developer.clickup.com>); the HTTP calls live in the app.

use serde::Deserialize;

use crate::tickets::{Source, Ticket, hex_color};

#[derive(Deserialize)]
struct TaskJson {
    id: String,
    #[serde(default)]
    custom_id: Option<String>,
    name: String,
    status: StatusJson,
    url: String,
    #[serde(default)]
    list: Option<NamedJson>,
    #[serde(default)]
    space: Option<IdJson>,
    #[serde(default)]
    priority: Option<PriorityJson>,
    #[serde(default)]
    tags: Vec<NamedJson>,
    /// ClickUp sends dates as strings of milliseconds.
    #[serde(default)]
    due_date: Option<String>,
}

#[derive(Deserialize)]
struct IdJson {
    id: String,
}

#[derive(Deserialize)]
struct PriorityJson {
    priority: String,
}

#[derive(Deserialize)]
struct StatusJson {
    status: String,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Deserialize)]
struct NamedJson {
    name: String,
}

impl From<TaskJson> for Ticket {
    fn from(task: TaskJson) -> Self {
        let priority = task.priority.map(|priority| priority.priority);
        Self {
            source: Source::ClickUp,
            id: task.id,
            custom_id: task.custom_id.filter(|id| !id.is_empty()),
            title: task.name,
            status: task.status.status,
            status_color: task.status.color.as_deref().and_then(hex_color),
            url: task.url,
            list: task.list.map(|list| list.name),
            space_id: task.space.map(|space| space.id),
            space: None,
            priority_rank: priority.as_deref().and_then(|priority| {
                PRIORITIES.iter().position(|known| *known == priority).and_then(|rank| u32::try_from(rank).ok())
            }),
            priority,
            tags: task.tags.into_iter().map(|tag| tag.name).collect(),
            due: task.due_date.and_then(|due| due.parse().ok()),
        }
    }
}

/// How many tasks ClickUp returns per page.
const PAGE_SIZE: usize = 100;

/// A page of `GET /team/{team_id}/task`: its tickets, and whether more pages follow.
pub fn parse_task_page(json: &str) -> Result<(Vec<Ticket>, bool), serde_json::Error> {
    #[derive(Deserialize)]
    struct Page {
        tasks: Vec<TaskJson>,
        #[serde(default)]
        last_page: Option<bool>,
    }
    let page: Page = serde_json::from_str(json)?;
    // ClickUp says whether this was the last page; without that, a full page (100) may have more.
    let more = page.last_page.map_or(page.tasks.len() >= PAGE_SIZE, |last| !last);
    Ok((page.tasks.into_iter().map(Ticket::from).collect(), more))
}

/// One task from `GET /task/{id}`.
pub fn parse_task(json: &str) -> Result<Ticket, serde_json::Error> {
    serde_json::from_str::<TaskJson>(json).map(Ticket::from)
}

/// The signed-in user's id, from `GET /user`.
pub fn parse_user_id(json: &str) -> Result<u64, serde_json::Error> {
    #[derive(Deserialize)]
    struct Body {
        user: User,
    }
    #[derive(Deserialize)]
    struct User {
        id: u64,
    }
    serde_json::from_str::<Body>(json).map(|body| body.user.id)
}

/// The workspaces the token can see, from `GET /team`, as (id, name).
pub fn parse_workspaces(json: &str) -> Result<Vec<(String, String)>, serde_json::Error> {
    #[derive(Deserialize)]
    struct Body {
        teams: Vec<Team>,
    }
    #[derive(Deserialize)]
    struct Team {
        id: String,
        name: String,
    }
    serde_json::from_str::<Body>(json).map(|body| body.teams.into_iter().map(|team| (team.id, team.name)).collect())
}

/// ClickUp's priorities, most pressing first.
pub const PRIORITIES: [&str; 4] = ["urgent", "high", "normal", "low"];

/// A workspace's spaces, from `GET /team/{team_id}/space`, as (id, name).
pub fn parse_spaces(json: &str) -> Result<Vec<(String, String)>, serde_json::Error> {
    #[derive(Deserialize)]
    struct Body {
        spaces: Vec<Space>,
    }
    #[derive(Deserialize)]
    struct Space {
        id: String,
        name: String,
    }
    serde_json::from_str::<Body>(json).map(|body| body.spaces.into_iter().map(|space| (space.id, space.name)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r##"{
        "tasks": [
            {"id": "86c1abc2", "custom_id": "ABC-236", "name": "Dark mode toggle",
             "status": {"status": "building", "color": "#7b68ee", "type": "custom"},
             "url": "https://app.clickup.com/t/86c1abc2", "list": {"id": "9", "name": "Inbox"},
             "assignees": [{"id": 1}]},
            {"id": "86c1abc3", "custom_id": null, "name": "Filter on every list page",
             "status": {"status": "ready", "color": "#d3d3d3"},
             "url": "https://app.clickup.com/t/86c1abc3"}
        ],
        "last_page": false
    }"##;

    #[test]
    fn a_page_of_tasks_becomes_tickets() {
        let (tickets, more) = parse_task_page(PAGE).expect("parses");
        assert!(more);
        assert_eq!(tickets[0].key(), "ABC-236");
        assert_eq!(tickets[0].status, "building");
        assert_eq!(tickets[0].list.as_deref(), Some("Inbox"));
        assert_eq!(tickets[1].key(), "86c1abc3", "without a custom id, ClickUp's id names it");
    }

    #[test]
    fn reads_priority_tags_and_due_date_and_filters_by_them() {
        let json = r##"{"id": "1", "name": "Pay out refunds", "status": {"status": "to do"}, "url": "u",
            "list": {"name": "Web"}, "priority": {"priority": "urgent", "color": "#f50000"},
            "tags": [{"name": "billing"}], "due_date": "1000"}"##;
        let ticket = parse_task(json).expect("parses");
        assert_eq!(
            (ticket.priority.as_deref(), ticket.tags.as_slice(), ticket.due),
            (Some("urgent"), &["billing".to_owned()][..], Some(1000))
        );
        assert_eq!(ticket.priority_rank, Some(0), "urgent is ClickUp's most pressing");
    }

    #[test]
    fn a_tickets_space_is_named_from_the_space_list() {
        let ticket =
            parse_task(r#"{"id": "1", "name": "n", "status": {"status": "to do"}, "url": "u", "space": {"id": "77"}}"#)
                .expect("parses");
        assert_eq!(ticket.space_id.as_deref(), Some("77"));
        let spaces = parse_spaces(r#"{"spaces": [{"id": "77", "name": "Shop", "private": false}]}"#).expect("parses");
        assert_eq!(spaces, [("77".to_owned(), "Shop".to_owned())]);
    }

    #[test]
    fn the_user_and_workspaces_are_read() {
        assert_eq!(parse_user_id(r#"{"user": {"id": 4242, "username": "j"}}"#).expect("parses"), 4242);
        let teams = parse_workspaces(r#"{"teams": [{"id": "9001", "name": "Acme", "members": []}]}"#).expect("parses");
        assert_eq!(teams, [("9001".to_owned(), "Acme".to_owned())]);
    }
}
