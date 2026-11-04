//! Tickets as Akyba needs them, from whichever tracker they come from (ClickUp, YouTrack): an id to
//! start an agent on, a title, and a status to show on the pane; and the Tickets panel's search,
//! filters and grouping. Each tracker's module turns its API's answers into these.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The tracker a ticket comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    ClickUp,
    YouTrack,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Self::ClickUp => "ClickUp",
            Self::YouTrack => "YouTrack",
        }
    }
}

/// One ticket, as the picker and the pane header show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub source: Source,
    /// The tracker's own id (`86c1abc2` in ClickUp, `2-15` in YouTrack), for API calls and links.
    pub id: String,
    /// The id people call the ticket by (`ABC-236`), when the tracker has one besides its own.
    pub custom_id: Option<String>,
    pub title: String,
    pub status: String,
    /// The status's colour in the tracker, `#rrggbb`.
    pub status_color: Option<String>,
    pub url: String,
    /// The list it's in (ClickUp); YouTrack has none.
    pub list: Option<String>,
    /// The space it's in (ClickUp) or its project (YouTrack): an id from the ticket, and its name
    /// once known.
    pub space_id: Option<String>,
    pub space: Option<String>,
    /// Its priority by the tracker's name for it (`urgent`, `Critical`), when it has one.
    pub priority: Option<String>,
    /// Where the priority sits among the tracker's, most pressing first, to order the filter.
    pub priority_rank: Option<u32>,
    pub tags: Vec<String>,
    /// When it's due, in milliseconds since 1970.
    pub due: Option<i64>,
}

impl Ticket {
    /// What the ticket is called: its custom id, else the tracker's id.
    pub fn key(&self) -> &str {
        self.custom_id.as_deref().unwrap_or(&self.id)
    }

    /// Whether a search matches its key, title or status, ignoring case.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || [self.key(), &self.title, &self.status].iter().any(|text| text.to_lowercase().contains(&query))
    }
}

/// `#rgb` or `#rrggbb` as `#rrggbb`; anything else isn't a colour Akyba can draw.
pub fn hex_color(color: &str) -> Option<String> {
    let digits = color.strip_prefix('#')?;
    if !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        6 => Some(color.to_lowercase()),
        3 => Some(format!("#{}", digits.chars().flat_map(|digit| [digit, digit]).collect::<String>().to_lowercase())),
        _ => None,
    }
}

/// When a ticket is due, as the filter offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Due {
    Overdue,
    NextWeek,
    NoDate,
}

impl Due {
    pub const ALL: [Self; 3] = [Self::Overdue, Self::NextWeek, Self::NoDate];

    pub fn name(self) -> &'static str {
        match self {
            Self::Overdue => "Overdue",
            Self::NextWeek => "Next 7 days",
            Self::NoDate => "No due date",
        }
    }

    /// Where a ticket falls, at `now` (milliseconds); `None` when due later than a week out.
    pub fn of(ticket: &Ticket, now: i64) -> Option<Self> {
        const WEEK: i64 = 7 * 24 * 60 * 60 * 1000;
        match ticket.due {
            None => Some(Self::NoDate),
            Some(due) if due < now => Some(Self::Overdue),
            Some(due) if due < now + WEEK => Some(Self::NextWeek),
            Some(_) => None,
        }
    }
}

/// Which tickets the Tickets panel shows. Within a kind any chosen value matches (to do or
/// review); across kinds all must (to do, and urgent). An empty kind lets everything through.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    pub statuses: BTreeSet<String>,
    pub priorities: BTreeSet<String>,
    pub spaces: BTreeSet<String>,
    pub lists: BTreeSet<String>,
    pub tags: BTreeSet<String>,
    pub due: BTreeSet<Due>,
}

impl Filter {
    /// How many values are chosen, for the filter button's badge.
    pub fn count(&self) -> usize {
        self.statuses.len()
            + self.priorities.len()
            + self.spaces.len()
            + self.lists.len()
            + self.tags.len()
            + self.due.len()
    }

    pub fn keeps(&self, ticket: &Ticket, now: i64) -> bool {
        let one_of = |chosen: &BTreeSet<String>, value: Option<&String>| {
            chosen.is_empty() || value.is_some_and(|value| chosen.contains(value))
        };
        one_of(&self.statuses, Some(&ticket.status))
            && one_of(&self.priorities, ticket.priority.as_ref())
            && one_of(&self.spaces, ticket.space.as_ref())
            && one_of(&self.lists, ticket.list.as_ref())
            && (self.tags.is_empty() || ticket.tags.iter().any(|tag| self.tags.contains(tag)))
            && (self.due.is_empty() || Due::of(ticket, now).is_some_and(|due| self.due.contains(&due)))
    }
}

/// Adds `value` to `set`, or takes it out if it's there.
pub fn toggle<T: Ord>(set: &mut BTreeSet<T>, value: T) {
    if !set.remove(&value) {
        set.insert(value);
    }
}

/// The priorities to filter by: those the tickets have and those chosen, most pressing first by
/// each tracker's own order; one chosen that no ticket has any more goes last.
pub fn priorities(tickets: &[Ticket], chosen: &BTreeSet<String>) -> Vec<String> {
    let mut ranked: Vec<(u32, String)> = Vec::new();
    for ticket in tickets {
        if let Some(priority) = &ticket.priority
            && !ranked.iter().any(|(_, known)| known == priority)
        {
            ranked.push((ticket.priority_rank.unwrap_or(u32::MAX), priority.clone()));
        }
    }
    for priority in chosen {
        if !ranked.iter().any(|(_, known)| known == priority) {
            ranked.push((u32::MAX, priority.clone()));
        }
    }
    // Stable, so priorities of the same rank keep the order they were first seen in.
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(_, priority)| priority).collect()
}

/// Groups tickets by status for the picker, keeping the order of first appearance.
pub fn by_status(tickets: &[Ticket]) -> Vec<(&str, Vec<&Ticket>)> {
    let mut groups: Vec<(&str, Vec<&Ticket>)> = Vec::new();
    for ticket in tickets {
        match groups.iter_mut().find(|(status, _)| *status == ticket.status) {
            Some((_, group)) => group.push(ticket),
            None => groups.push((&ticket.status, vec![ticket])),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(key: &str, status: &str, priority: Option<(&str, u32)>) -> Ticket {
        Ticket {
            source: Source::YouTrack,
            id: key.to_owned(),
            custom_id: Some(key.to_owned()),
            title: format!("Work on {key}"),
            status: status.to_owned(),
            status_color: None,
            url: String::new(),
            list: None,
            space_id: None,
            space: None,
            priority: priority.map(|(name, _)| name.to_owned()),
            priority_rank: priority.map(|(_, rank)| rank),
            tags: Vec::new(),
            due: None,
        }
    }

    #[test]
    fn search_looks_at_the_key_the_title_and_the_status() {
        let ticket = ticket("ABC-236", "Ready", None);
        assert!(ticket.matches("abc-236"));
        assert!(ticket.matches("work on"));
        assert!(ticket.matches("READY"));
        assert!(!ticket.matches("dark mode"));
    }

    #[test]
    fn filters_combine_within_a_kind_and_across_kinds() {
        let mut ticket = ticket("ABC-1", "to do", Some(("urgent", 0)));
        ticket.tags = vec!["billing".to_owned()];
        ticket.due = Some(1000);
        let mut filter = Filter::default();
        assert!(filter.keeps(&ticket, 0));
        toggle(&mut filter.statuses, "to do".to_owned());
        toggle(&mut filter.statuses, "review".to_owned());
        assert!(filter.keeps(&ticket, 0), "any chosen status matches");
        toggle(&mut filter.priorities, "low".to_owned());
        assert!(!filter.keeps(&ticket, 0), "every kind must match");
        toggle(&mut filter.priorities, "low".to_owned());
        filter.due.insert(Due::Overdue);
        assert!(filter.keeps(&ticket, 5000) && !filter.keeps(&ticket, 0));
        assert_eq!(filter.count(), 3);
    }

    #[test]
    fn tickets_group_by_status_in_order() {
        let tickets = [ticket("A-1", "building", None), ticket("A-2", "ready", None), ticket("A-3", "building", None)];
        let groups = by_status(&tickets);
        assert_eq!(groups.iter().map(|(status, _)| *status).collect::<Vec<_>>(), ["building", "ready"]);
        assert_eq!(groups[0].1.len(), 2);
    }

    #[test]
    fn priorities_go_most_pressing_first_with_chosen_ones_kept() {
        let tickets = [
            ticket("A-1", "open", Some(("Normal", 3))),
            ticket("A-2", "open", Some(("Critical", 1))),
            ticket("A-3", "open", Some(("Normal", 3))),
            ticket("A-4", "open", None),
        ];
        let chosen = BTreeSet::from(["Show-stopper".to_owned()]);
        assert_eq!(priorities(&tickets, &chosen), ["Critical", "Normal", "Show-stopper"]);
    }

    #[test]
    fn colours_are_read_in_both_lengths() {
        assert_eq!(hex_color("#7B68EE").as_deref(), Some("#7b68ee"));
        assert_eq!(hex_color("#fa0").as_deref(), Some("#ffaa00"));
        assert_eq!(hex_color("red"), None);
        assert_eq!(hex_color("#12345"), None);
    }
}
